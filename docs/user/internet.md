# Playing over the internet

MCPanel itself never opens a network port. Your server listens on its port (TCP 25565 by
default, and UDP 19132 for Bedrock); how friends reach it is up to you:

- **Same network:** use this PC's LAN IP address.
- **Port forwarding:** forward the port on your router to this PC. MCPanel does not change
  router or firewall settings.
- **playit.gg:** a free tunnel service. The server Overview shows whether the playit program
  is installed and running. Install it from playit.gg, start it, then create a *Minecraft
  Java* tunnel to `127.0.0.1:<your port>` (and a *Minecraft Bedrock* UDP tunnel for
  Geyser) in the playit.gg dashboard.

MCPanel does not create playit tunnels for you and does not start the playit agent: playit.gg
offers no supported API for creating tunnels, and starting the agent publishes your
tunnels.
