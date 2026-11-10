# Online play on Windows (bo2mp)

Everyone plays through one **server** (`codlegacy-master`): a relay and lobby list,
not a game server. It needs no game files and no GPU. The host's own game runs
the match; the server forwards packets and answers where friends are.

## Pick where the server runs

| Where | How | Friends need |
|---|---|---|
| **This PC** (port-forward) | `host-online.cmd` in the project folder. It makes the certificates once (`online\certs`) and serves on UDP 4433. Forward UDP 4433 on the router to this PC for friends outside the house. | your public IP, and `online\certs\ca.pem` |
| **A Windows VPS** (no port-forward) | copy `online\vps-kit` (made by `run-bo2.cmd`: `codlegacy-master.exe` and its installer) to the VPS over Remote Desktop, then right-click `install.cmd` > Run as administrator. It starts with Windows, restarts if it stops, opens UDP 4433 in Windows Firewall, and prints the `online.cfg` lines and the `ca.pem` to hand out. Copy a newer `codlegacy-master.exe` and run it again to update; `install.cmd -Uninstall` removes it. | the VPS IP, and its `ca.pem` |
| **A Linux VPS** (no port-forward) | on the VPS itself, one command: `curl -fsSL https://raw.githubusercontent.com/sero7k/bo2-multiplayer-rebuild/main/scripts/install-master-vps.sh \| sudo bash` (Debian, Ubuntu, Fedora, Rocky, Alma; run it again to update). It prints the `online.cfg` lines and the `ca.pem` to hand out. From a Linux machine with the clone instead: `cargo xtask master install root@IP` ([`MASTER.md`](MASTER.md)) | the VPS IP, and its `ca.pem` |
| **A friend's PC** | they run `host-online.cmd` | their IP, and their `ca.pem` |

"P2P" in practice is the first row: one player's PC is the server for
everyone.

## Point the game at it: `online.cfg`

`run-bo2.cmd` writes a template the first time; fill in `server` and run it
again:

```ini
server=127.0.0.1:4433        # the server's address (this PC: 127.0.0.1)
ca=online\certs\ca.pem       # the server's ca.pem (relative to the project)
name=iw4l-prod               # the label its certificate carries (leave as is)
```

`run-bo2.cmd` turns these into `CODLEGACY_MASTER_ADDR`, `CODLEGACY_MASTER_SERVER_NAME` and
`CODLEGACY_MASTER_CA_CERT` in `play\.env`. An empty `server` plays offline.

## Friends

* Each PC has an 8-digit **friend code** (Options > ACCOUNT, and FRIENDS on
  the main menu), made once and saved in `settings.cfg`.
* FRIENDS > ADD FRIEND: type a friend's code. Each lobby-list refresh (every
  15 s) asks the server about them: ONLINE, IN A LOBBY (map, mode, players),
  IN A MATCH or OFFLINE. Del removes one.
* The server knows a code only while its player is connected or browsed in
  the last 45 seconds; nothing is stored on disk.

## Hosting and joining

* **Host:** set up a lobby as usual and press START. With a server in
  `online.cfg`, the game opens a room on it and starts the match there; you
  load the map with everyone in the room. If the server cannot host (down,
  refused, or no room within 15 s) the match starts on this PC offline.
* **Join a friend:** FRIENDS, pick a friend who shows IN A LOBBY, press
  Enter. You load in when they press START.
* **Find a match:** FIND A MATCH on the main menu. A playlist filter
  (CORE, PARTY GAMES; every mode in Black Ops II's gametypestable, with its
  playlist picture and its rules from the game's gamesettings), plus
  BRAINROT 24/7 (Shipment only; Team Deathmatch unless other modes are
  ticked). SEARCH opens a matchmaking lobby: it searches, says how many
  games it found, joins the fullest open one that fits (the next if a join
  does not take), then shows the room filling until its host starts. None
  found: it keeps looking, and HOST A MATCH opens your own lobby in that
  playlist for the next player searching.
* **Join anyone:** MULTIPLAYER > FIND A LOBBY lists every lobby on the
  server (open ones first); Enter joins an open one.
* Every player needs the map's game installed (Black Ops II, Call of Duty 4,
  World at War, ...); the server only lets in players who have it.

## Playing with a friend without a server (P2P custom games)

No server, no `online.cfg`, no certificates:

1. The host: MULTIPLAYER > CUSTOM GAME > **PLAY ONLINE WITH FRIENDS**. His
   lobby opens to the internet and its **invite code** goes on the
   clipboard (COPY INVITE CODE copies it again). He sends it to his friends
   any way he likes.
2. A friend copies the code, then MULTIPLAYER > **JOIN WITH CODE**. He waits
   in the host's room (Create a Class, Barracks and Options open over it);
   the host sees him in his lobby's list.
3. The host picks the map and mode and presses START MATCH: everyone plays
   it. CLOSE ONLINE LOBBY, or leaving the lobby, ends it for everyone.

How it gets through: the host's game runs the server itself, on **UDP port
28970** (`CODLEGACY_P2P_PORT` picks another). It asks the router to open that
port (UPnP, else NAT-PMP) and asks a public STUN server for its public
address (`stun.l.google.com`, `stun1.l.google.com`, `stun.cloudflare.com`;
`CODLEGACY_P2P_STUN` takes a comma list instead). The code holds the host's
public, LAN and IPv6 addresses, a secret and his certificate's fingerprint:
a connection without the secret is closed at once, and a friend's game only
accepts the host's own certificate. The friend's game tries every address at
once (a friend on the same network goes straight over the LAN) and a few
ports either side of the public one. The code shows no address in the game.

When it does not get through within five seconds the friend is told so, with
the port: the host forwards that UDP port on his router to his PC (or turns
UPnP on), and the friend joins again. Two routers that both refuse
unrequested packets and have no UPnP cannot meet without a forwarded port.
Each player still needs the map's game installed.

Relay games and P2P games are the same game underneath: the same lobby,
content checks, guest-gun stand-ins and match flow; only where the service
runs differs.

## Not done yet

Zombies through the server; a lobby screen showing who else is in the room
while waiting for the host.
