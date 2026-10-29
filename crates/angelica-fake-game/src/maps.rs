//! Game maps for tests, so a plugin's tests reach the game's objects without
//! writing a map: [`EXAMPLE`] is given to [`crate::TestHost::use_map`] before the
//! plugin is approved.

/// A small game map shaped like the released one, with constant offsets the
/// scripted game lays its objects out by. Its roots and fields have the
/// released map's names, so a plugin's queries read the same way: the
/// player's character (`player { name, data.level }`), the zone, statuses,
/// the camera, the game's windows, the events `action_used` and
/// `chat_message_added`, the game's text and letter case, and the actions
/// `use_action` and `answer_yes_no`, in the released map's domains.
pub const EXAMPLE: [(&str, &str); 12] = [
    (
        "entries/excel.toml",
        r#"
[function."excel.hash_table_page_row_resolver.store_row"]
locator = { signature = "C3" }
abi = { args = ["ptr", "ptr"], ret = "ptr" }
thread = "any"
"#,
    ),
    (
        "entries/text.toml",
        r#"
[function."text.utf8_string.to_upper"]
locator = { signature = "C3" }
abi = { args = ["ptr", "u8", "u8", "u8", "ptr"], ret = "ptr" }
thread = "any"

[function."text.utf8_string.to_lower"]
locator = { signature = "C3" }
abi = { args = ["ptr"], ret = "ptr" }
thread = "any"
"#,
    ),
    (
        "entries/test.toml",
        r#"
[global."test.local_player"]
locator = { signature = "C3" }
type = "ptr"

[global."test.game_main"]
locator = { signature = "C3" }
type = "struct"

[field."test.game_object.name"]
of = "test.game_object"
type = "struct"
offset = 0x30

[field."test.game_object.position_x"]
of = "test.game_object"
type = "f32"
offset = 0xB0

[field."test.game_object.position_y"]
of = "test.game_object"
type = "f32"
offset = 0xB4

[field."test.game_object.position_z"]
of = "test.game_object"
type = "f32"
offset = 0xB8

[field."test.character.data"]
of = "test.character"
type = "struct"
offset = 0x1A0

[field."test.character_data.class_job"]
of = "test.character_data"
type = "u8"
offset = 0x2A

[field."test.character_data.level"]
of = "test.character_data"
type = "u8"
offset = 0x2B

[field."test.character_data.health"]
of = "test.character_data"
type = "u32"
offset = 0x0C

[field."test.character_data.max_health"]
of = "test.character_data"
type = "u32"
offset = 0x10

[field."test.character_data.mana"]
of = "test.character_data"
type = "u32"
offset = 0x14

[field."test.character_data.max_mana"]
of = "test.character_data"
type = "u32"
offset = 0x18

[field."test.battle_chara.status_manager"]
of = "test.battle_chara"
type = "struct"
offset = 0x23B0

[field."test.status_manager.statuses"]
of = "test.status_manager"
type = "struct"
offset = 0x8

[field."test.status.status_id"]
of = "test.status"
type = "u16"
offset = 0x0

[field."test.status.remaining_time"]
of = "test.status"
type = "f32"
offset = 0x4

[field."test.game_main.current_territory_type"]
of = "test.game_main"
type = "u32"
offset = 0x4108

[function."test.use_action"]
locator = { signature = "C3" }
abi = { args = ["ptr", "u32", "u32", "u64"], ret = "bool" }
thread = "main"

[global."test.action_manager"]
locator = { signature = "C3" }
type = "struct"

[function."test.print_message"]
locator = { signature = "C3" }
abi = { args = ["ptr", "u16", "ptr", "ptr", "i32", "u8"], ret = "u32" }
thread = "main"

[field."test.utf8_string.string_ptr"]
of = "test.utf8_string"
type = "ptr"
offset = 0x0

[global."test.stage"]
locator = { signature = "C3" }
type = "ptr"

[field."test.stage.unit_manager"]
of = "test.stage"
type = "ptr"
offset = 0x20

[field."test.unit_manager.units"]
of = "test.unit_manager"
type = "struct"
offset = 0x10

[field."test.unit_manager.unit_count"]
of = "test.unit_manager"
type = "u16"
offset = 0x8

[field."test.window.name"]
of = "test.window"
type = "struct"
offset = 0x8

[field."test.window.values"]
of = "test.window"
type = "ptr"
offset = 0x178

[field."test.window.value_count"]
of = "test.window"
type = "u16"
offset = 0x1E2

[field."test.atk_value.kind"]
of = "test.atk_value"
type = "u8"
offset = 0x0

[field."test.atk_value.text"]
of = "test.atk_value"
type = "ptr"
offset = 0x8

[function."test.fire_callback_int"]
locator = { signature = "C3" }
abi = { args = ["ptr", "i32"], ret = "bool" }
thread = "main"

[field."test.window.layout"]
of = "test.window"
type = "ptr"
offset = 0x28

[field."test.widget.nodes"]
of = "test.widget"
type = "ptr"
offset = 0x10

[field."test.widget.node_count"]
of = "test.widget"
type = "u16"
offset = 0x18

[field."test.node.id"]
of = "test.node"
type = "u32"
offset = 0x14

[field."test.node.kind"]
of = "test.node"
type = "u16"
offset = 0x90

[field."test.node.flags"]
of = "test.node"
type = "u32"
offset = 0x9C

[function."test.set_visible"]
locator = { signature = "C3" }
abi = { args = ["ptr", "u8"], ret = "void" }
thread = "main"

[field."test.text_node.text"]
of = "test.text_node"
type = "struct"
offset = 0xD0

[function."test.set_text"]
locator = { signature = "C3" }
abi = { args = ["ptr", "ptr"], ret = "void" }
thread = "main"

[global."test.camera_manager"]
locator = { signature = "C3" }
type = "struct"

[field."test.camera_manager.camera"]
of = "test.camera_manager"
type = "ptr"
offset = 0x0

[field."test.camera.max_distance"]
of = "test.camera"
type = "f32"
offset = 0x12C

[field."test.camera.distance"]
of = "test.camera"
type = "f32"
offset = 0x124
"#,
    ),
    (
        "types/test.toml",
        r#"
[types."test.game_object"]
doc = "An object."
domain = "character"

[types."test.game_object".fields.name]
doc = "The name."
text = 64

[types."test.game_object".fields.position_x]
doc = "East-west."
domain = "location"

[types."test.game_object".fields.position_y]
doc = "Height."
domain = "location"

[types."test.game_object".fields.position_z]
doc = "North-south."
domain = "location"

[types."test.character"]
doc = "A character."
domain = "character"
extends = "test.game_object"

[types."test.character".fields.data]
doc = "Its numbers."
embed = "test.character_data"

[types."test.character_data"]
doc = "Numbers."
domain = "character"

[types."test.character_data".fields.class_job]
doc = "Class."
sheet = "ClassJob"

[types."test.character_data".fields.level]
doc = "Level."

[types."test.character_data".fields.health]
doc = "HP."

[types."test.character_data".fields.max_health]
doc = "Maximum HP."

[types."test.character_data".fields.mana]
doc = "MP."

[types."test.character_data".fields.max_mana]
doc = "Maximum MP."

[types."test.battle_chara"]
doc = "A battle character."
domain = "character"
extends = "test.character"

[types."test.battle_chara".fields.status_manager]
doc = "Its statuses."
embed = "test.status_manager"
domain = "statuses"

[types."test.status_manager"]
doc = "Statuses."
domain = "statuses"

[types."test.status_manager".fields.statuses]
doc = "The slots."
array = { of = "test.status", length = 60, stride = 16, present = "status_id" }

[types."test.status"]
doc = "A status."
domain = "statuses"

[types."test.status".fields.status_id]
doc = "The status."
sheet = "Status"

[types."test.status".fields.remaining_time]
doc = "Seconds."
unit = "s"

[types."test.game_main"]
doc = "The game."
domain = "location"

[types."test.stage"]
doc = "The stage."
domain = "windows"

[types."test.stage".fields.unit_manager]
doc = "The windows' manager."
ref = "test.unit_manager"

[types."test.unit_manager"]
doc = "The windows' manager."
domain = "windows"

[types."test.unit_manager".fields.unit_count]
doc = "How many."

[types."test.unit_manager".fields.windows]
doc = "The windows."
at = "test.unit_manager.units"
array = { of = "test.window", count = "unit_count", most = 256, stride = 8, pointers = true }

[types."test.window"]
doc = "A window."
domain = "windows"

[types."test.window".fields.name]
doc = "Its name."
text = 32

[types."test.window".fields.value_count]
doc = "How many values."

[types."test.window".fields.values]
doc = "Its values."
array = { of = "test.atk_value", count = "value_count", most = 64, stride = 16 }

[types."test.window".fields.layout]
doc = "Its elements as its layout builds them."
ref = "test.widget"

[types."test.widget"]
doc = "A window's elements."
domain = "windows"

[types."test.widget".fields.element_count]
doc = "How many elements."
at = "test.widget.node_count"

[types."test.widget".fields.elements]
doc = "The elements."
at = "test.widget.nodes"
array = { of = "test.node", count = "element_count", most = 64, stride = 8, pointers = true }

[types."test.node"]
doc = "An element of a window."
domain = "windows"

[types."test.node".fields.name]
doc = "Its name."
at = "test.node.id"
element = "test.window.name"

[types."test.node".fields.kind]
doc = "What it is."
enum = "test.node_kind"

[types."test.node".fields.visible]
doc = "Whether it is shown."
at = "test.node.flags"
bit = 20
set = { call = "test.set_visible", hold = true }

[types."test.text_node"]
doc = "A text element."
domain = "windows"
extends = "test.node"
variant = { field = "kind", values = ["text"] }

[types."test.text_node".fields.text]
doc = "Its text."
pointer = "test.utf8_string.string_ptr"
text = 512
set = { call = "test.set_text", hold = true }

[types."test.atk_value"]
doc = "A value."
domain = "windows"

[types."test.atk_value".fields.kind]
doc = "Its kind."

[types."test.atk_value".fields.text]
doc = "Its text."
text = 256

[types."test.utf8_string"]
doc = "A string."
domain = "chat"

[types."test.utf8_string".fields.text]
doc = "Its text."
at = "test.utf8_string.string_ptr"
text = 4096

[types."test.camera_manager"]
doc = "The cameras."
domain = "camera"

[types."test.camera_manager".fields.camera]
doc = "The camera."
ref = "test.camera"

[types."test.camera"]
doc = "A camera."
domain = "camera"

[types."test.camera".fields.max_distance]
doc = "The zoom limit."
unit = "yalms"
set = { range = [1.5, 100.0] }

[types."test.camera".fields.distance]
doc = "The zoom."
unit = "yalms"

[types."test.game_main".fields.current_territory_type]
doc = "The zone."
sheet = "TerritoryType"
"#,
    ),
    (
        "domains.toml",
        r#"
[domains.character]
doc = "The character."
class = "you"
read = { en = "Sees your character", ru = "Видит вашего персонажа" }

[domains.location]
doc = "Where."
class = "you"
read = { en = "Sees where you are", ru = "Видит, где вы" }

[domains.statuses]
doc = "Statuses."
class = "you"
read = { en = "Sees your statuses", ru = "Видит ваши эффекты" }

[domains.actions]
doc = "Actions."
class = "you"
read = { en = "Sees the actions you use", ru = "Видит действия, которые вы используете" }
act = { en = "Uses actions for you", ru = "Использует действия за вас" }

[domains.teleport]
doc = "Teleports."
class = "you"
read = { en = "Sees where you teleport", ru = "Видит, куда вы телепортируетесь" }
act = { en = "Teleports you to aetherytes", ru = "Телепортирует вас к эфиритам" }

[domains.fitting]
doc = "The Try On window."
class = "you"
read = { en = "Sees what you try on", ru = "Видит, что вы примеряете" }
act = { en = "Opens items in the Try On window", ru = "Открывает предметы в окне примерки" }

[domains.windows]
doc = "The game's windows."
class = "others"
read = { en = "Sees the game's windows", ru = "Видит окна игры" }
act = { en = "Answers the game's dialogs for you", ru = "Отвечает на диалоги игры за вас" }
change = { en = "Hides parts of the game's windows", ru = "Скрывает части окон игры" }

[domains.chat]
doc = "The chat log."
class = "others"
read = { en = "Sees your chat log", ru = "Видит ваш чат" }
change = { en = "Hides messages in your chat log", ru = "Скрывает сообщения в вашем чате" }

[domains.text]
doc = "The game's text."
class = "display"
read = { en = "Sees the game's text", ru = "Видит тексты игры" }
change = { en = "Changes the game's text", ru = "Меняет тексты игры" }

[domains.camera]
doc = "The camera."
class = "display"
read = { en = "Sees your camera", ru = "Видит вашу камеру" }
change = { en = "Changes your camera's zoom limits", ru = "Меняет пределы приближения камеры" }
"#,
    ),
    (
        "actions.toml",
        r#"
[actions.use_action]
doc = "Uses an action as a hotbar slot does."
call = "test.use_action"
domain = "actions"
rate = 3
ok = true
when = [{ path = "player", says = "you are not in the world" }]

[actions.answer_yes_no]
doc = "Answers the yes or no dialog."
call = "test.fire_callback_int"
domain = "windows"
when = [{ path = "windows.unit_manager.windows[name = \"SelectYesno\"]", says = "no yes or no dialog is open" }]

[actions.answer_yes_no.args.window]
arg = 0
object = "windows.unit_manager.windows[name = \"SelectYesno\"]"

[actions.answer_yes_no.args.answer]
arg = 1
doc = "Yes or no."
enum = "test.yes_no"

[actions.use_action.args.manager]
arg = 0
global = "test.action_manager"

[actions.use_action.args.kind]
arg = 1
doc = "The kind."
enum = "test.action_kind"

[actions.use_action.args.action]
arg = 2
doc = "The id."

[actions.use_action.args.target]
arg = 3
doc = "The target."
default = 0xE0000000
"#,
    ),
    (
        "events.toml",
        r#"
[events.action_used]
doc = "The player asked to use an action."
at = "test.use_action"
domain = "actions"
whose = "you"

[events.action_used.args.kind]
doc = "The kind."
arg = 1
enum = "test.action_kind"

[events.action_used.args.action]
doc = "The id."
arg = 2

[events.action_used.args.target]
doc = "The target."
arg = 3

[events.chat_message_added]
doc = "The game adds a message to the chat log."
at = "test.print_message"
domain = "chat"
whose = "others"

[events.chat_message_added.args.kind]
doc = "The log kind."
arg = 1
mask = 0x7F

[events.chat_message_added.args.sender]
doc = "The sender."
arg = 2
ref = "test.utf8_string"

[events.chat_message_added.args.message]
doc = "The message."
arg = 3
ref = "test.utf8_string"

[events.chat_message_added.outcomes.hide]
doc = "The message is not added."
returns = 0

[events.sheet_text]
doc = "A text cell of a sheet row the game builds."
at = "excel.hash_table_page_row_resolver.store_row"
derived = "sheet_cell"
domain = "text"
rules = "text-table-v1"
whose = "you"

[events.sheet_text.args.sheet]
doc = "The sheet."
from = "sheet"
text = 256

[events.sheet_text.args.row]
doc = "The row."
from = "row"

[events.sheet_text.args.column]
doc = "The column."
from = "column"

[events.sheet_text.args.text]
doc = "The game's text."
from = "text"
text = 4096

[events.sheet_text.outcomes.replace]
doc = "The cell shows the rule's text."
text = 65535

[events.letter_upper]
doc = "A letter the conversion to capitals meets."
at = "text.utf8_string.to_upper"
derived = "case_letter"
domain = "text"
whose = "you"

[events.letter_upper.args.letter]
doc = "The letter."
from = "letter"
text = 4

[events.letter_upper.outcomes.becomes]
doc = "The conversion gives the rule's letter."
text = 4
same_length = "letter"

[events.letter_lower]
doc = "A letter the conversion to small letters meets."
at = "text.utf8_string.to_lower"
derived = "case_letter"
domain = "text"
whose = "you"

[events.letter_lower.args.letter]
doc = "The letter."
from = "letter"
text = 4

[events.letter_lower.outcomes.becomes]
doc = "The conversion gives the rule's letter."
text = 4
same_length = "letter"
"#,
    ),
    (
        "enums/test.node_kind.toml",
        "doc = \"Kinds of element.\"\nvalues = { res = 1, text = 3 }\n",
    ),
    (
        "windows/_Exp.toml",
        r#"
window = "_Exp"
doc = "The experience bar."
layout = "ui/uld/Exp.uld"

[elements.window]
kind = "other"
doc = "The whole bar."

[elements.text]
kind = "text"
doc = "The experience text."

[[layouts]]
fingerprint = "50e3bcde560501b1"
elements = { window = 1, text = 4 }
"#,
    ),
    (
        "enums/test.yes_no.toml",
        "doc = \"Answers.\"\nvalues = { yes = 0, no = 1 }\n",
    ),
    (
        "enums/test.action_kind.toml",
        "doc = \"Kinds.\"\nvalues = { action = 1, item = 2, general_action = 5, mount = 13 }\n",
    ),
    (
        "roots.toml",
        r#"
[roots.player]
doc = "The player."
global = "test.local_player"
type = "test.battle_chara"
whose = "you"

[roots.game]
doc = "The game."
global = "test.game_main"
type = "test.game_main"
whose = "you"

[roots.windows]
doc = "The windows."
global = "test.stage"
type = "test.stage"
whose = "you"

[roots.camera]
doc = "The cameras."
global = "test.camera_manager"
type = "test.camera_manager"
whose = "you"
"#,
    ),
];

/// The player, the bags, and the camera as the game map describes them,
/// with constant offsets of the scripted build.
pub const PLAYER_AND_BAGS: [(&str, &str); 11] = [
    (
        "entries/character.toml",
        r#"
[global."character.local_player"]
locator = { signature = "C3" }
type = "ptr"


[field."character.character.data"]
of = "character.character"
type = "struct"
offset = 0x1A0


[field."character.character_data.level"]
of = "character.character_data"
type = "u8"
offset = 0x2B

"#,
    ),
    (
        "entries/object.toml",
        r#"
[field."object.game_object.name"]
of = "object.game_object"
type = "struct"
offset = 0x30

"#,
    ),
    (
        "entries/inventory.toml",
        r#"
[global."inventory.inventory_manager.instance"]
locator = { signature = "C3" }
type = "struct"


[field."inventory.inventory_manager.inventories"]
of = "inventory.inventory_manager"
type = "ptr"
offset = 0x1E08


[field."inventory.inventory_container.items"]
of = "inventory.inventory_container"
type = "ptr"
offset = 0x08


[field."inventory.inventory_container.kind"]
of = "inventory.inventory_container"
type = "u32"
offset = 0x10


[field."inventory.inventory_container.size"]
of = "inventory.inventory_container"
type = "u32"
offset = 0x14


[field."inventory.inventory_item.item_id"]
of = "inventory.inventory_item"
type = "u32"
offset = 0x10


[field."inventory.inventory_item.quantity"]
of = "inventory.inventory_item"
type = "u32"
offset = 0x14

"#,
    ),
    (
        "entries/camera.toml",
        r#"
[global."camera.control.instance"]
locator = { signature = "C3" }
type = "struct"


[field."camera.camera_manager.camera"]
of = "camera.camera_manager"
type = "ptr"
offset = 0x0


[field."camera.camera.max_distance"]
of = "camera.camera"
type = "f32"
offset = 0x12C
"#,
    ),
    (
        "types/object.toml",
        r#"
[types."object.game_object"]
doc = "An object."
domain = "character"


[types."object.game_object".fields.name]
doc = "Its name."
text = 64

"#,
    ),
    (
        "types/character.toml",
        r#"
[types."character.character"]
doc = "A character."
domain = "character"
extends = "object.game_object"


[types."character.character".fields.data]
doc = "Numbers."
embed = "character.character_data"


[types."character.character_data"]
doc = "Numbers."
domain = "character"


[types."character.character_data".fields.level]
doc = "Level."


[types."character.battle_chara"]
doc = "A battle character."
domain = "character"
extends = "character.character"

"#,
    ),
    (
        "types/inventory.toml",
        r#"
[types."inventory.inventory_manager"]
doc = "Every container."
domain = "inventory"


[types."inventory.inventory_manager".fields.containers]
doc = "The containers."
at = "inventory.inventory_manager.inventories"
array = { of = "inventory.inventory_container", length = 84, stride = 32, partition = { field = "kind", domains = { armoury = ["armoury_head"] } } }


[types."inventory.inventory_container"]
doc = "One container."
domain = "inventory"


[types."inventory.inventory_container".fields.kind]
doc = "Which container this is."
enum = "inventory.container_kind"


[types."inventory.inventory_container".fields.size]
doc = "How many slots it has."


[types."inventory.inventory_container".fields.items]
doc = "The slots."
array = { of = "inventory.inventory_item", count = "size", most = 200, stride = 72 }


[types."inventory.inventory_item"]
doc = "One slot."
domain = "inventory"


[types."inventory.inventory_item".fields.item]
doc = "The item."
at = "inventory.inventory_item.item_id"


[types."inventory.inventory_item".fields.quantity]
doc = "How many."

"#,
    ),
    (
        "types/camera.toml",
        r#"
[types."camera.camera_manager"]
doc = "The cameras."
domain = "camera"


[types."camera.camera_manager".fields.camera]
doc = "The camera."
ref = "camera.camera"


[types."camera.camera"]
doc = "A camera."
domain = "camera"


[types."camera.camera".fields.max_distance]
doc = "The zoom limit."
set = { range = [1.5, 100.0] }
"#,
    ),
    (
        "enums/inventory.container_kind.toml",
        "doc = \"Containers.\"\nvalues = { bag_1 = 0, bag_2 = 1, armoury_head = 3201 }\n",
    ),
    (
        "domains.toml",
        r#"
[domains.character]
doc = "The player's character."
class = "you"
read = { en = "Sees your character", ru = "Видит вашего персонажа" }

[domains.inventory]
doc = "The items the player carries."
class = "you"
read = { en = "Sees the items you carry", ru = "Видит предметы, которые вы носите" }

[domains.armoury]
doc = "The armoury chest."
class = "you"
read = { en = "Sees your armoury chest", ru = "Видит ваш оружейный сундук" }

[domains.camera]
doc = "The camera."
class = "display"
read = { en = "Sees your camera", ru = "Видит вашу камеру" }
change = { en = "Changes your camera's zoom limits", ru = "Меняет пределы приближения камеры" }
"#,
    ),
    (
        "roots.toml",
        r#"
[roots.player]
doc = "The player's character."
global = "character.local_player"
type = "character.battle_chara"
whose = "you"

[roots.inventory]
doc = "The player's items."
global = "inventory.inventory_manager.instance"
type = "inventory.inventory_manager"
whose = "you"

[roots.camera]
doc = "The cameras."
global = "camera.control.instance"
type = "camera.camera_manager"
whose = "you"
"#,
    ),
];
