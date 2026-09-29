# Players

The **Players** tab lists who is online, who played before (with play time), operators,
the whitelist and bans.

- While the server runs, MCPanel sends the matching command (`op`, `whitelist add`, `ban`,
  `kick`, …) and shows the server's reply.
- While it is stopped, MCPanel edits `ops.json`, `whitelist.json` and the ban lists
  directly, keeping fields it does not know.

Names are resolved to UUIDs through the server's user cache, Mojang's profile service
(online mode) or the offline UUID (offline mode).

New servers on Minecraft 26.3 and later have the whitelist **on** by default — add your
friends to the whitelist or turn it off.

Bedrock players who join through Floodgate appear with a `.` in front of their name (see
[Bedrock crossplay](bedrock.md)).
