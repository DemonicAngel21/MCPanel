# Servers

## Server software

| Software | Content | Notes |
|---|---|---|
| Vanilla | datapacks | Official Mojang server |
| Paper, Purpur | plugins | Plugins from Modrinth, Hangar, SpigotMC and GeyserMC |
| Fabric, Quilt | mods | Installed natively (no installer); Quilt also runs most Fabric mods |
| NeoForge, Forge | mods | The official installer runs inside the server folder and is removed afterwards |

Old Forge versions (before 1.17) need Java 8 to run.

## Java and memory

Each server has its own Java runtime and memory range (Settings tab of the server). MCPanel
checks that the runtime is new enough for the Minecraft version. JVM arguments can be added,
but only safe kinds are accepted: `-D` system properties, `-XX` options, `-Xss`/`-Xmn` and
module options such as `--add-opens=…`. Options that could start other programs or load code
from elsewhere are refused.

## server.properties

The **Properties** tab edits `server.properties` without losing comments, order or unknown
keys. Settings that do not apply to your Minecraft version are marked. Secret values
(RCON password, management-server secret) are write-only: MCPanel shows that they are set,
never their value — also in the text editor, where they appear as `<hidden by MCPanel>` and
are kept when you save.

## Files

The **Files** tab browses the server folder: edit text files, upload, download, rename,
move, copy, zip and unzip. Deleted files go to a trash inside the server folder. Highly
sensitive files (the Floodgate key, Geyser login tokens) are shown locked and cannot be
opened, exported, renamed, moved or copied.

## Import an existing server

**Import** points MCPanel at a server folder you already have. MCPanel detects the software
and version. It never runs scripts from an imported folder.

## Templates

Templates (survival, friends, creative, hardcore, performance) pre-fill settings, a backup
schedule, automatic restart and suggested plugins. They never add JVM flags.
