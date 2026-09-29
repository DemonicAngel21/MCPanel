# Getting started

## Install

Run the MCPanel installer. It installs for your Windows user only and never asks for
administrator rights. MCPanel needs a 64-bit Java runtime to run servers; it finds the
runtimes already on your PC (the **Java runtimes** page lists them, and you can add one
by path).

## Create your first server

1. Click **+** next to *Servers* (or start from a template on **Templates**).
2. Choose the software:
   - **Vanilla** — the official Mojang server.
   - **Paper** / **Purpur** — faster servers with plugin support.
   - **Fabric**, **Quilt**, **NeoForge**, **Forge** — mod loaders.
3. Choose the Minecraft version, a name and the memory. MCPanel picks a compatible Java
   runtime.
4. MCPanel downloads the server from its official source and checks every file against
   the published hash before using it.

## The Minecraft EULA

Minecraft servers only start after you accept Mojang's End User License Agreement. MCPanel
shows a banner with a link to the EULA and an **I accept** button. It never accepts the
EULA for you.

## Start, stop, restart

Use the buttons at the top of a server. **Stop** asks the server to save and shut down;
if it does not stop within the configured time you can force-stop it. The **Console** tab
shows the live output and lets you type server commands.

Closing the MCPanel window keeps it running in the system tray so your servers stay
online. Quit from the tray icon; MCPanel then stops the servers gracefully (Settings →
*Stop timeout when quitting*).

## Joining your server

On the same PC use `localhost` (the Overview tab shows the exact address). Other devices on
your network use this PC's LAN IP address and the server port. For friends elsewhere, see
[Playing over the internet](internet.md).
