# Plugins and mods

The **Plugins** tab (Paper/Purpur) or **Mods** tab (Fabric, Quilt, NeoForge, Forge) shows
what is installed and lets you search and install more.

## Sources

| Source | Verification |
|---|---|
| Modrinth | SHA-512 hash from Modrinth |
| Hangar (PaperMC) | SHA-256 hash from Hangar |
| GeyserMC | SHA-256 hash from GeyserMC (Geyser and Floodgate) |
| SpigotMC (through Spiget) | **No hash exists** — shown as *unverified*; only the latest version of free resources can be installed |

MCPanel only offers versions that fit your server software and Minecraft version, installs
required dependencies, refuses client-only mods and checks every jar's descriptor
(`plugin.yml`, `fabric.mod.json`, `mods.toml`) before placing it.

## While the server runs

Windows locks loaded jars, so changes made while the server runs are queued and applied
when it stops or before it next starts.

## Updates and unknown files

**Check for updates** compares installed versions with their source. Jars you added
yourself are identified by their hash on Modrinth when possible. Disabled files move to
`.mcpanel/disabled` inside the server folder; replaced and removed files go to the trash.
