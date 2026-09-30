# Playing over the internet

MCPanel itself never opens a network port. Your server listens on its port (TCP 25565 by
default, and UDP 19132 for Bedrock); how friends reach it is up to you:

- **Same network:** use this PC's LAN IP address.
- **Port forwarding:** forward the port on your router to this PC. MCPanel does not change
  router or firewall settings.
- **playit.gg:** a free tunnel service. See below.

## playit.gg

Open **Playit.gg** in the left navigation.

1. **Install** the playit program from playit.gg if MCPanel says it is missing (MCPanel
   uses its `playitd` agent program).
2. **Link with playit.gg**: playit.gg opens in your browser; sign in or create a free
   account and approve "MCPanel". MCPanel keeps the agent's key in the Windows Credential
   Manager. Your other playit agents and tunnels are not changed.
3. MCPanel **runs its agent** while MCPanel is open (you can turn off "Start the agent when
   MCPanel starts" and start/stop it by hand). Tunnels are online only while it runs.
4. **New tunnel**: pick a server; the port is filled in. Choose *Minecraft Java* (and a
   *Minecraft Bedrock* tunnel for Geyser). playit.gg assigns the public address (for
   example `name.joinmc.link`) within a few seconds; copy it for your friends. The
   server's Overview shows it too.
5. **Edit** (name, server/port), **disable/enable** or **delete** tunnels from the list.
   Tunnels of other playit agents are listed but managed on playit.gg.
6. **Unlink** stops the agent and forgets its key; remove the agent on playit.gg
   (Account → Agents) if you no longer need it.

MCPanel uses playit.gg's web API, which playit.gg does not officially document for other
programs (it is the API playit's own Minecraft plugin uses). If playit.gg changes it,
MCPanel shows the error and the playit.gg dashboard still works.

The **Installed playit service** card controls the playit program's own background
service (separate from MCPanel's agent).
