# Troubleshooting

| What you see | What it means | What to do |
|---|---|---|
| *The Minecraft EULA has not been accepted* | Minecraft refuses to start without it | Read the EULA and click **I accept** |
| *Port … is already in use by …* | Another program (or server) uses the port | Stop it or change `server-port` on the Properties tab |
| *The Bedrock port … (UDP) is already in use* | Another program uses Geyser's port | Change the Bedrock port on the Bedrock tab |
| *Java … is too old* | The server needs a newer Java | Pick another runtime in the server's Settings, or install one |
| *The selected Java runtime does not accept one of the JVM arguments* | A JVM flag is not supported by that Java | Remove it in the server's Settings |
| *The server ran out of memory* | Not enough maximum memory | Increase the memory in the server's Settings |
| *The server stopped responding … watchdog* | A tick (often a plugin) blocked the server | See the suspects in Recent crashes |
| *Geyser needs ViaVersion on this server* | Geyser targets a newer Minecraft version | Install ViaVersion from the Bedrock tab or update the server |
| *The encryption key is not on this computer* | Encrypted backups need the key | Settings → Backup encryption → Import Recovery Kit |
| *This server kept running while MCPanel was closed* | The server outlived MCPanel | Wait for it to stop, or force-stop it |

If you need help from someone else, save a **Diagnostics** file (Overview → Details) and
share it with them.
