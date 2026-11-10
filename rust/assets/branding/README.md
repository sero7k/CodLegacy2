# Call of Duty: Legacy branding

Original artwork for the project (drawn for it; no game's logo or lettering).

- `legacy_emblem.svg`: the emblem. A steel shield holds an L, cut by an orange rank chevron, with three stars.
- `legacy_logo.svg`: the wordmark, with the emblem on the left, "CALL OF DUTY" above an orange rule, and "LEGACY" below.
- `legacy_discord.png`: 1024x1024, for Discord Rich Presence.
- `legacy_logo.png`: 1600x600, the wordmark.
- `legacy_icon_256.png`: 256x256, an icon.

To render the PNGs again: `rsvg-convert -w 1024 -h 1024 legacy_emblem.svg -o legacy_discord.png`. The wordmark's text uses DejaVu Sans Condensed Bold.

## Discord Rich Presence setup (`ui::discord`)

1. At https://discord.com/developers/applications, make a **New Application** named `Call of Duty: Legacy`. Discord shows that name as "Playing Call of Duty: Legacy".
2. Under **Rich Presence → Art Assets**, upload `legacy_discord.png` with the key `legacy_logo`.
3. Copy the application's **Application ID** and give it to the game in one of these ways:
   - set `BO2MP_DISCORD_APP_ID=<id>`;
   - put the ID alone in `discord_app_id.txt` beside the game (or in the folder it starts in);
   - write it into `ui::discord::APP_ID`.

`BO2MP_DISCORD=0` turns Rich Presence off. The log lines start `discord:`.
