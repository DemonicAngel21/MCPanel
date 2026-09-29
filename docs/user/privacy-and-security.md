# Privacy and security

## What MCPanel sends

MCPanel has **no telemetry** and no accounts. It contacts the internet only for what you
ask it to do: server software and version information from the official providers
(Mojang, PaperMC, PurpurMC, FabricMC, QuiltMC, NeoForged, MinecraftForge), plugins and mods
from Modrinth, Hangar, GeyserMC and SpigotMC (through the Spiget API), and player profiles
from Mojang. All downloads use HTTPS from known hosts and are checked against published
hashes where they exist.

## What MCPanel stores

- Its database, logs and console captures in `%LOCALAPPDATA%\MCPanel`.
- Your servers in their own folders; Minecraft's files stay the source of truth.
- The backup encryption key in the Windows Credential Manager (your account, this PC).

## What MCPanel protects

- Highly sensitive files (Floodgate key, Geyser login tokens) cannot be opened, exported,
  renamed, moved or copied, and never appear in logs, events or diagnostics.
- Secret `server.properties` values are write-only in the UI.
- File access is limited to the server folder: MCPanel refuses paths that leave it and does
  not follow links or junctions.
- MCPanel never runs scripts from imported folders, never opens a network port, runs
  without administrator rights and launches servers without a shell.
