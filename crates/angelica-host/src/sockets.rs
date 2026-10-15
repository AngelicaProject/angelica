//! Plugins' WebSocket connections. Each runs on a thread of its own; what
//! arrives waits for the plugin's next frame, and a connection whose
//! plugin falls behind waits rather than dropping messages.
#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    io::{Read, Write},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel},
    },
    time::{Duration, Instant},
};

use tungstenite::{
    Message as Frame,
    client::IntoClientRequest,
    protocol::{CloseFrame, WebSocketConfig, frame::coding::CloseCode},
};

use crate::error::{Error, Kind, Result};

/// The most connections a plugin keeps open.
pub const MOST_OPEN: usize = 4;
/// The largest message sent or received.
pub const MOST_MESSAGE: usize = 16 * 1024 * 1024;
/// The most messages waiting to be sent on one connection.
pub const MOST_OUTGOING: usize = 64;
/// The most events waiting for one plugin's next frame; a connection with
/// more to deliver waits.
pub const MOST_WAITING: usize = 256;
/// The most events delivered to a plugin in one frame.
pub const MOST_A_FRAME: usize = 64;
/// The most subprotocols a plugin offers.
pub const MOST_PROTOCOLS: usize = 8;
/// How long a connection without traffic sleeps between looks.
const IDLE: Duration = Duration::from_millis(10);
/// How long a closing connection waits for the server's close.
const CLOSING: Duration = Duration::from_secs(5);

/// A message of a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
}

/// What happened on one of a plugin's connections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Happened {
    /// The handshake succeeded, with the subprotocol the server chose.
    Opened {
        socket: u32,
        protocol: Option<String>,
    },
    Received {
        socket: u32,
        message: Message,
    },
    /// The connection ended: the server's close code and reason, or 1006
    /// and what went wrong when it ended without one.
    Closed {
        socket: u32,
        code: u16,
        reason: String,
    },
}

/// A byte stream a connection runs over.
pub trait Stream: Read + Write + Send {
    /// Makes reads return `WouldBlock` instead of waiting.
    ///
    /// # Errors
    /// The socket's error.
    fn set_nonblocking(&self, nonblocking: bool) -> std::io::Result<()>;
}

impl Stream for std::net::TcpStream {
    fn set_nonblocking(&self, nonblocking: bool) -> std::io::Result<()> {
        std::net::TcpStream::set_nonblocking(self, nonblocking)
    }
}

impl Stream for native_tls::TlsStream<std::net::TcpStream> {
    fn set_nonblocking(&self, nonblocking: bool) -> std::io::Result<()> {
        self.get_ref().set_nonblocking(nonblocking)
    }
}

/// Opens the stream of a new connection, on the connection's thread.
pub type Connect = Box<dyn FnOnce() -> Result<Box<dyn Stream>> + Send>;

/// Opens the stream to a `wss://` URL for a plugin's HTTP client.
pub type Dial = Arc<dyn Fn(&crate::http::Client, &str) -> Result<Box<dyn Stream>> + Send + Sync>;

enum Outgoing {
    Send(Message),
    Close,
}

struct Connection {
    outgoing: SyncSender<Outgoing>,
    stop: Arc<AtomicBool>,
}

struct Plugin {
    open: BTreeMap<u32, Connection>,
    sender: SyncSender<Happened>,
    receiver: Receiver<Happened>,
}

pub struct Sockets {
    plugins: Mutex<BTreeMap<String, Plugin>>,
    next: AtomicU32,
    dial: Dial,
}

impl Default for Sockets {
    /// Connections through [`crate::http::Client::connect`].
    fn default() -> Self {
        Self::dialing(Arc::new(|client: &crate::http::Client, url: &str| {
            let stream: Box<dyn Stream> = Box::new(client.connect(url)?);
            Ok(stream)
        }))
    }
}

fn invalid(problem: impl Into<String>) -> Error {
    Error::new(Kind::InvalidArgument, problem)
}

impl Sockets {
    /// Connections whose streams `dial` opens; for the scripted game.
    #[must_use]
    pub fn dialing(dial: Dial) -> Self {
        Self {
            plugins: Mutex::new(BTreeMap::new()),
            next: AtomicU32::new(0),
            dial,
        }
    }

    /// What opens connections' streams.
    #[must_use]
    pub fn dial(&self) -> Dial {
        self.dial.clone()
    }

    /// Opens a connection of `plugin` to `url` through the stream `connect`
    /// makes, offering `protocols`, with an `authorization` header when
    /// given; returns its id. The handshake runs on the connection's
    /// thread, and its outcome arrives as [`Happened::Opened`] or
    /// [`Happened::Closed`].
    ///
    /// # Errors
    /// Too many connections, and invalid protocols or authorization.
    pub fn open(
        &self,
        plugin: &str,
        url: &str,
        protocols: &[String],
        authorization: Option<&str>,
        connect: Connect,
    ) -> Result<u32> {
        if protocols.len() > MOST_PROTOCOLS
            || protocols.iter().any(|protocol| {
                protocol.is_empty()
                    || protocol.len() > 64
                    || !protocol
                        .bytes()
                        .all(|byte| byte.is_ascii_graphic() && byte != b',')
            })
        {
            return Err(invalid(format!(
                "at most {MOST_PROTOCOLS} subprotocols of 1 to 64 visible ASCII characters without commas"
            )));
        }
        let mut request = url
            .into_client_request()
            .map_err(|e| invalid(format!("{url}: {e}")))?;
        if !protocols.is_empty() {
            let value = protocols
                .join(", ")
                .parse()
                .map_err(|_| invalid("a subprotocol is not a header value"))?;
            request
                .headers_mut()
                .insert("sec-websocket-protocol", value);
        }
        if let Some(authorization) = authorization {
            if authorization.len() > crate::http::MAX_HEADER_VALUE {
                return Err(invalid("the authorization has at most 1024 bytes"));
            }
            let value = authorization
                .parse()
                .map_err(|_| invalid("the authorization is not a header value"))?;
            request.headers_mut().insert("authorization", value);
        }
        let mut plugins = self.plugins.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = plugins.entry(plugin.to_owned()).or_insert_with(|| {
            let (sender, receiver) = sync_channel(MOST_WAITING);
            Plugin {
                open: BTreeMap::new(),
                sender,
                receiver,
            }
        });
        if entry.open.len() >= MOST_OPEN {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!("a plugin keeps at most {MOST_OPEN} connections open"),
            ));
        }
        let socket = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (outgoing, queued) = sync_channel(MOST_OUTGOING);
        let stop = Arc::new(AtomicBool::new(false));
        let events = entry.sender.clone();
        let stopped = stop.clone();
        std::thread::Builder::new()
            .name(format!("angelica socket {plugin} {socket}"))
            .spawn(move || {
                let (code, reason) = run(socket, request, connect, &queued, &events, &stopped);
                deliver(
                    &events,
                    &stopped,
                    Happened::Closed {
                        socket,
                        code,
                        reason,
                    },
                );
            })
            .map_err(|e| Error::new(Kind::Unavailable, format!("no thread: {e}")))?;
        entry.open.insert(socket, Connection { outgoing, stop });
        Ok(socket)
    }

    /// Queues `message` on `plugin`'s connection `socket`.
    ///
    /// # Errors
    /// An unknown or closed connection, a message over [`MOST_MESSAGE`],
    /// and a full queue.
    pub fn send(&self, plugin: &str, socket: u32, message: Message) -> Result<()> {
        let size = match &message {
            Message::Text(text) => text.len(),
            Message::Binary(bytes) => bytes.len(),
        };
        if size > MOST_MESSAGE {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!("a message has at most {MOST_MESSAGE} bytes"),
            ));
        }
        self.queue(plugin, socket, Outgoing::Send(message))
    }

    /// Closes `plugin`'s connection `socket` once what was queued is sent;
    /// [`Happened::Closed`] follows.
    ///
    /// # Errors
    /// An unknown or closed connection.
    pub fn close(&self, plugin: &str, socket: u32) -> Result<()> {
        self.queue(plugin, socket, Outgoing::Close)
    }

    fn queue(&self, plugin: &str, socket: u32, outgoing: Outgoing) -> Result<()> {
        let plugins = self.plugins.lock().unwrap_or_else(PoisonError::into_inner);
        let connection = plugins
            .get(plugin)
            .and_then(|entry| entry.open.get(&socket))
            .ok_or_else(|| Error::new(Kind::NotFound, format!("no open connection {socket}")))?;
        match connection.outgoing.try_send(outgoing) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(Error::new(
                Kind::LimitExceeded,
                format!("the connection still sends {MOST_OUTGOING} earlier messages"),
            )),
            Err(TrySendError::Disconnected(_)) => Err(Error::new(
                Kind::NotFound,
                format!("connection {socket} is closed"),
            )),
        }
    }

    /// What happened on `plugin`'s connections since the last call, at most
    /// [`MOST_A_FRAME`]; a closed connection is forgotten as its close is
    /// taken.
    pub fn take(&self, plugin: &str) -> Vec<Happened> {
        let mut plugins = self.plugins.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(entry) = plugins.get_mut(plugin) else {
            return Vec::new();
        };
        let mut taken = Vec::new();
        while taken.len() < MOST_A_FRAME {
            match entry.receiver.try_recv() {
                Ok(happened) => {
                    if let Happened::Closed { socket, .. } = &happened {
                        entry.open.remove(socket);
                    }
                    taken.push(happened);
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        taken
    }

    /// Whether `plugin` has connections open or events waiting.
    pub fn busy(&self, plugin: &str) -> bool {
        self.plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(plugin)
            .is_some_and(|entry| !entry.open.is_empty())
    }

    /// Closes a stopped plugin's connections and forgets them.
    pub fn remove_plugin(&self, plugin: &str) {
        let removed = self
            .plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(plugin);
        for connection in removed
            .into_iter()
            .flat_map(|entry| entry.open.into_values())
        {
            connection.stop.store(true, Ordering::Release);
        }
    }
}

/// Passes `happened` to the plugin, waiting while its queue is full, until
/// the connection is stopped.
fn deliver(events: &SyncSender<Happened>, stop: &AtomicBool, mut happened: Happened) -> bool {
    loop {
        match events.try_send(happened) {
            Ok(()) => return true,
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(back)) => {
                if stop.load(Ordering::Acquire) {
                    return false;
                }
                happened = back;
                std::thread::sleep(IDLE);
            }
        }
    }
}

/// Sends what tungstenite holds, for at most a second.
fn send_queued(ws: &mut Socket) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(1) {
        match ws.flush() {
            Err(error) if would_block(&error) => std::thread::sleep(IDLE),
            _ => return,
        }
    }
}

fn would_block(error: &tungstenite::Error) -> bool {
    matches!(error, tungstenite::Error::Io(io) if io.kind() == std::io::ErrorKind::WouldBlock)
}

/// The close code of a connection that ended without the server's close.
const ABNORMAL: u16 = 1006;

type Socket = tungstenite::WebSocket<Box<dyn Stream>>;

/// Runs one connection until it ends; the close code and reason.
fn run(
    socket: u32,
    request: tungstenite::handshake::client::Request,
    connect: Connect,
    queued: &Receiver<Outgoing>,
    events: &SyncSender<Happened>,
    stop: &AtomicBool,
) -> (u16, String) {
    let (ws, protocol) = match handshake(request, connect) {
        Ok(opened) => opened,
        Err(reason) => return (ABNORMAL, reason),
    };
    if !deliver(events, stop, Happened::Opened { socket, protocol }) {
        return (ABNORMAL, "the plugin stopped".into());
    }
    pump(socket, ws, queued, events, stop)
}

/// Connects and shakes hands; the connection, made non-blocking, and the
/// subprotocol the server chose.
fn handshake(
    request: tungstenite::handshake::client::Request,
    connect: Connect,
) -> std::result::Result<(Socket, Option<String>), String> {
    let stream = connect().map_err(|error| error.message)?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(MOST_MESSAGE))
        .max_frame_size(Some(MOST_MESSAGE));
    let (ws, response) = tungstenite::client::client_with_config(request, stream, Some(config))
        .map_err(|error| format!("the handshake failed: {error}"))?;
    let protocol = response
        .headers()
        .get("sec-websocket-protocol")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    ws.get_ref()
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    Ok((ws, protocol))
}

/// Starts closing with `code`.
fn start_closing(ws: &mut Socket, code: CloseCode, reason: &str) {
    let _ = ws.close(Some(CloseFrame {
        code,
        reason: reason.into(),
    }));
}

/// Writes what the plugin queued, until it asks to close; whether there
/// was anything.
///
/// # Errors
/// The reason the connection failed.
fn write_queued(
    ws: &mut Socket,
    queued: &Receiver<Outgoing>,
    closing: &mut bool,
) -> std::result::Result<bool, String> {
    let mut busy = false;
    while !*closing {
        match queued.try_recv() {
            Ok(Outgoing::Send(message)) => {
                busy = true;
                let frame = match message {
                    Message::Text(text) => Frame::text(text),
                    Message::Binary(bytes) => Frame::binary(bytes),
                };
                if let Err(error) = ws.write(frame)
                    && !would_block(&error)
                {
                    return Err(error.to_string());
                }
            }
            Err(TryRecvError::Empty) => break,
            // The plugin closed the connection or let go of it.
            Ok(Outgoing::Close) | Err(TryRecvError::Disconnected) => {
                *closing = true;
                start_closing(ws, CloseCode::Normal, "");
            }
        }
    }
    Ok(busy)
}

/// Moves messages both ways until the connection ends; the close code and
/// reason.
fn pump(
    socket: u32,
    mut ws: Socket,
    queued: &Receiver<Outgoing>,
    events: &SyncSender<Happened>,
    stop: &AtomicBool,
) -> (u16, String) {
    let mut closing = false;
    let mut closing_since: Option<Instant> = None;
    loop {
        if stop.load(Ordering::Acquire) && !closing {
            closing = true;
            start_closing(&mut ws, CloseCode::Away, "the plugin stopped");
        }
        let mut busy = match write_queued(&mut ws, queued, &mut closing) {
            Ok(busy) => busy,
            Err(reason) => return (ABNORMAL, reason),
        };
        if closing && closing_since.get_or_insert_with(Instant::now).elapsed() > CLOSING {
            return (ABNORMAL, "the server did not close".into());
        }
        if let Err(error) = ws.flush()
            && !would_block(&error)
        {
            return (ABNORMAL, error.to_string());
        }
        let received = match ws.read() {
            Ok(Frame::Text(text)) => Some(Message::Text(text.as_str().to_owned())),
            Ok(Frame::Binary(bytes)) => Some(Message::Binary(bytes.to_vec())),
            Ok(Frame::Close(frame)) => {
                // Both sides have sent their close, or tungstenite queued
                // the answer to the server's: the close handshake is
                // complete once that is sent, whether or not the server
                // ends the TCP connection soon.
                send_queued(&mut ws);
                return frame.map_or((1005, String::new()), |frame| {
                    (u16::from(frame.code), frame.reason.as_str().to_owned())
                });
            }
            Ok(Frame::Ping(_) | Frame::Pong(_) | Frame::Frame(_)) => {
                busy = true;
                None
            }
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return (1000, String::new());
            }
            Err(error) if would_block(&error) => None,
            Err(error) => return (ABNORMAL, error.to_string()),
        };
        if let Some(message) = received {
            busy = true;
            if !deliver(events, stop, Happened::Received { socket, message }) {
                closing = true;
            }
        }
        if !busy {
            std::thread::sleep(IDLE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    /// Chooses the first subprotocol the client offers.
    #[allow(clippy::result_large_err, clippy::unnecessary_wraps)] // the callback type tungstenite asks for
    fn first_protocol(
        request: &tungstenite::handshake::server::Request,
        mut response: tungstenite::handshake::server::Response,
    ) -> std::result::Result<
        tungstenite::handshake::server::Response,
        tungstenite::handshake::server::ErrorResponse,
    > {
        if let Some(offered) = request.headers().get("sec-websocket-protocol") {
            let first = offered.to_str().unwrap().split(',').next().unwrap().trim();
            response
                .headers_mut()
                .insert("sec-websocket-protocol", first.parse().unwrap());
        }
        Ok(response)
    }

    /// A server on this computer that answers each message with the same
    /// message, and closes when told `bye`.
    fn echo_server() -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept_hdr(stream, first_protocol).unwrap();
            loop {
                match ws.read() {
                    Ok(Frame::Text(text)) if text.as_str() == "bye" => {
                        ws.close(Some(CloseFrame {
                            code: CloseCode::Normal,
                            reason: "bye".into(),
                        }))
                        .unwrap();
                    }
                    Ok(message @ (Frame::Text(_) | Frame::Binary(_))) => ws.send(message).unwrap(),
                    Ok(_) => {}
                    Err(_) => return,
                }
            }
        });
        (format!("ws://{address}/echo"), server)
    }

    fn plain(url: &str) -> Connect {
        let address = url
            .trim_start_matches("ws://")
            .split('/')
            .next()
            .unwrap()
            .to_owned();
        Box::new(move || {
            let stream: Box<dyn Stream> = Box::new(
                TcpStream::connect(address)
                    .map_err(|e| Error::new(Kind::Network, e.to_string()))?,
            );
            Ok(stream)
        })
    }

    /// What happens on the plugin's connections until `until` holds or
    /// five seconds pass.
    fn wait(sockets: &Sockets, until: impl Fn(&[Happened]) -> bool) -> Vec<Happened> {
        let started = Instant::now();
        let mut all = Vec::new();
        while started.elapsed() < Duration::from_secs(5) {
            all.extend(sockets.take("p"));
            if until(&all) {
                return all;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("only {all:?}");
    }

    #[test]
    fn a_connection_opens_echoes_and_closes() {
        let (url, server) = echo_server();
        let sockets = Sockets::default();
        let socket = sockets
            .open(
                "p",
                &url,
                &["chat.v1".into(), "chat.v0".into()],
                Some("Bearer x"),
                plain(&url),
            )
            .unwrap();
        sockets
            .send("p", socket, Message::Text("hello".into()))
            .unwrap();
        sockets
            .send("p", socket, Message::Binary(vec![1, 2, 3]))
            .unwrap();
        sockets
            .send("p", socket, Message::Text("bye".into()))
            .unwrap();
        let all = wait(&sockets, |all| {
            all.iter()
                .any(|happened| matches!(happened, Happened::Closed { .. }))
        });
        assert_eq!(
            all,
            [
                Happened::Opened {
                    socket,
                    protocol: Some("chat.v1".into())
                },
                Happened::Received {
                    socket,
                    message: Message::Text("hello".into())
                },
                Happened::Received {
                    socket,
                    message: Message::Binary(vec![1, 2, 3])
                },
                Happened::Closed {
                    socket,
                    code: 1000,
                    reason: "bye".into()
                },
            ]
        );
        assert!(!sockets.busy("p"), "a closed connection is forgotten");
        assert_eq!(
            sockets
                .send("p", socket, Message::Text("late".into()))
                .unwrap_err()
                .kind,
            Kind::NotFound
        );
        server.join().unwrap();
    }

    #[test]
    fn a_failed_connection_closes_with_the_reason() {
        let sockets = Sockets::default();
        let socket = sockets
            .open(
                "p",
                "wss://example.com/",
                &[],
                None,
                Box::new(|| {
                    Err(Error::new(
                        Kind::PermissionDenied,
                        "example.com is not listed",
                    ))
                }),
            )
            .unwrap();
        let all = wait(&sockets, |all| !all.is_empty());
        assert_eq!(
            all,
            [Happened::Closed {
                socket,
                code: 1006,
                reason: "example.com is not listed".into()
            }]
        );
    }

    #[test]
    fn connections_are_few_and_checked() {
        let sockets = Sockets::default();
        let never = || -> Connect {
            Box::new(|| {
                std::thread::sleep(Duration::from_millis(200));
                Err(Error::new(Kind::Network, "no"))
            })
        };
        for _ in 0..MOST_OPEN {
            sockets
                .open("p", "wss://example.com/", &[], None, never())
                .unwrap();
        }
        assert_eq!(
            sockets
                .open("p", "wss://example.com/", &[], None, never())
                .unwrap_err()
                .kind,
            Kind::LimitExceeded
        );
        sockets
            .open("q", "wss://example.com/", &[], None, never())
            .unwrap();
        for protocols in [vec![String::new()], vec!["a,b".into()], vec!["x".into(); 9]] {
            assert_eq!(
                sockets
                    .open("r", "wss://example.com/", &protocols, None, never())
                    .unwrap_err()
                    .kind,
                Kind::InvalidArgument
            );
        }
        assert_eq!(
            sockets
                .send("q", 999, Message::Binary(vec![0; MOST_MESSAGE + 1]))
                .unwrap_err()
                .kind,
            Kind::LimitExceeded
        );
        sockets.remove_plugin("p");
        assert!(!sockets.busy("p"));
    }

    /// Needs the internet: a public echo server over TLS, reached through
    /// the plugin's client as in the game.
    #[test]
    #[ignore = "needs the internet"]
    fn echoes_over_tls_on_the_internet() {
        let sockets = Sockets::default();
        let client = crate::http::Client::new(vec!["echo.websocket.org".into()], "p", "1.0.0");
        let dial = sockets.dial();
        let url = "wss://echo.websocket.org/";
        let socket = sockets
            .open("p", url, &[], None, Box::new(move || dial(&client, url)))
            .unwrap();
        wait(&sockets, |all| {
            all.iter()
                .any(|happened| matches!(happened, Happened::Opened { .. }))
        });
        sockets
            .send("p", socket, Message::Text("angelica".into()))
            .unwrap();
        wait(&sockets, |all| {
            all.contains(&Happened::Received {
                socket,
                message: Message::Text("angelica".into()),
            })
        });
        sockets.close("p", socket).unwrap();
        let all = wait(&sockets, |all| {
            all.iter()
                .any(|happened| matches!(happened, Happened::Closed { .. }))
        });
        assert!(
            matches!(all.last(), Some(Happened::Closed { code: 1000, .. })),
            "{all:?}"
        );
    }
}
