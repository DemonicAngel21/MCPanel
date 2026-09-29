# Playing over the internet

MCPanel itself never opens a network port. Your server listens on its port (TCP 25565 by
default, and UDP 19132 for Bedrock); how friends reach it is up to you:

- **Same network:** use this PC's LAN IP address.
- **Port forwarding:** forward the port on your router to this PC. MCPanel does not change
  router or firewall settings.
- **playit.gg:** a free tunnel service. See below.

## playit.gg

The *Internet access* panel (server Overview and Settings) works with the official playit
program:

1. **Install** the playit program from playit.gg (the panel links to the download page).
2. **Start agent** starts the playit background service. Stopping it takes every tunnel of
   this agent offline; starting it publishes them again.
3. **Link account** appears when the agent is not linked to a playit.gg account yet. MCPanel
   runs `playit setup` and opens playit.gg in your browser, where you approve the agent. The
   agent's secret goes straight from playit to its service; MCPanel never sees or stores it.
4. **Create the tunnel on playit.gg:** add a *Minecraft Java* tunnel with local address
   `127.0.0.1:<your port>` (and a *Minecraft Bedrock* tunnel for Geyser).
5. **Paste the public address** (e.g. `name.joinmc.link`) into the panel. MCPanel shows it
   with the server so you can copy it for your friends.

MCPanel cannot create, list or change playit tunnels: playit.gg has no public API for that.
The only interfaces are its private web API and the agent's internal connection, which are
not documented for other programs and can change at any time.
