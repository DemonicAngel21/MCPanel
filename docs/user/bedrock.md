# Bedrock crossplay

The **Bedrock** tab lets Minecraft: Bedrock Edition players (phones, consoles, Windows)
join your Java server through **Geyser**. **Floodgate** lets them use their Xbox account
instead of also owning Java Edition.

## Supported servers

| Software | Geyser source | Notes |
|---|---|---|
| Paper, Purpur (1.20.5+) | GeyserMC | Older Minecraft versions than Geyser supports need **ViaVersion**; MCPanel suggests it and detects Geyser's warning |
| Fabric | Modrinth | Only the newest Minecraft version Geyser supports (Fabric API is installed too) |
| NeoForge | Modrinth | Only the newest Minecraft version Geyser supports |

Vanilla, Forge and Quilt are not supported by Geyser.

## Setting up

Choose Floodgate (recommended), ViaVersion if suggested, and the Bedrock port (UDP, default
19132), then **Set up Geyser**. MCPanel writes only the port and sign-in method into
Geyser's config; Geyser completes the rest on first start. If another program already
uses the UDP port, the server does not start and MCPanel tells you which program it is.

**Test Bedrock connection** asks the running server's Bedrock listener for its status, the
same way the Bedrock game lists servers.

## Connecting

In Bedrock: *Play → Servers → Add Server*, this PC's address and the Bedrock port. Windows
may ask to allow Java through the firewall for UDP.

## The Floodgate key

Floodgate's `key.pem` lets anyone who has it impersonate Bedrock players on your server.
MCPanel only shows whether it exists; it never displays, exports or uploads it.
