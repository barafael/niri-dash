<!-- Written by `NIRI_DASH_BOOK=bless cargo test --lib book` (src/book.rs). Don't edit. -->

<!-- ANCHOR: index -->
| Name | Group | Shows | Default size | Shapes |
|---|---|---|---|---|
| [`clock`](time.md#clock) | Time | Digital, or analog when round | 4×2 | rectangular, round |
| [`calendar`](time.md#calendar) | Time | Month view | 4×4 | rectangular, round |
| [`weather`](time.md#weather) | Time | Now and the next days (Open-Meteo) | 4×2 | rectangular, round |
| [`timer`](time.md#timer) | Time | Countdown with a ring; scroll to set, presets when bigger | 3×2 | round, rectangular |
| [`stopwatch`](time.md#stopwatch) | Time | Laps; a seconds dial when round, the lap list when taller | 3×3 | round, rectangular |
| [`sleep-timer`](time.md#sleep-timer) | Time | Count down to suspend, hibernate or power off (like sleepy) | 4×3 | rectangular, round |
| [`pomodoro`](time.md#pomodoro) | Time | Focus sessions and breaks; a long break every few sessions | 3×2 | round, rectangular |
| [`windows`](activity.md#windows) | Activity | Open windows by workspace; click to focus | 5×5 | rectangular |
| [`claude-sessions`](activity.md#claude-sessions) | Activity | Running Claude Code sessions; click to focus | 4×3 | rectangular, round |
| [`notifications`](activity.md#notifications) | Activity | Notification history | 4×5 | rectangular |
| [`clipboard`](activity.md#clipboard) | Activity | Records what you copy while it's on the dashboard, in memory only | 4×4 | rectangular |
| [`claude-usage`](activity.md#claude-usage) | Activity | Plan, 5-hour window, tokens today | 4×3 | rectangular, round |
| [`mascot`](activity.md#mascot) | Activity | Schraubi, Zolli or Verschnitti; says what you tell it | 4×2 | rectangular |
| [`notes`](activity.md#notes) | Activity | A note that keeps what you type; name it to have several | 3×3 | rectangular |
| [`about`](activity.md#about) | Activity | Which version this is, its license, and what it is made with | 2×2 | rectangular |
| [`niri-dash-info`](activity.md#niri-dash-info) | Activity | Version, source, commit and build: what this niri-dash is made of | 4×4 | rectangular |
| [`media`](media.md#media) | Media | Now playing (MPRIS) | 5×2 | rectangular |
| [`system-monitor`](system.md#system-monitor) | System | CPU, memory, network, temperature | 4×3 | rectangular, round |
| [`battery`](system.md#battery) | System | Charge, power profile; device batteries and sleep blockers when bigger | 3×2 | rectangular, round |
| [`memory`](system.md#memory) | System | RAM and swap, history, biggest apps | 4×3 | rectangular, round |
| [`disks`](system.md#disks) | System | Space per filesystem, read and write | 4×2 | rectangular, round |
| [`network`](system.md#network) | System | Download and upload, history | 4×2 | rectangular, round |
| [`gpu`](system.md#gpu) | System | GPU load, video memory, temperature | 4×2 | rectangular, round |
| [`privacy`](system.md#privacy) | System | Who uses the microphone, camera or screen; stop screen sharing | 3×2 | rectangular, round |
| [`wifi`](devices.md#wifi) | Devices | Networks, connect, radio on/off | 3×4 | rectangular, round |
| [`bluetooth`](devices.md#bluetooth) | Devices | Devices, connect, adapter on/off | 3×3 | rectangular, round |
| [`removable-drives`](devices.md#removable-drives) | Devices | USB drives and SD cards: mount, open, eject | 3×2 | rectangular |
| [`phone`](devices.md#phone) | Devices | KDE Connect: battery, ring to find it, send files | 3×2 | rectangular, round |
| [`volume`](controls.md#volume) | Controls | Speaker and microphone; outputs and apps when bigger | 3×3 | rectangular, round |
| [`brightness`](controls.md#brightness) | Controls | Screen backlight | 3×1 | rectangular, round |
| [`tray`](controls.md#tray) | Controls | Status icons and their menus | 4×1 | rectangular |
| [`session`](controls.md#session) | Controls | Lock, suspend, log out, power off | 4×1 | rectangular |
| [`keyboard-layout`](controls.md#keyboard-layout) | Controls | Current layout; click to switch, bigger shows all | 2×1 | rectangular, round |
| [`color-picker`](controls.md#color-picker) | Controls | Pick a pixel anywhere; hex copied, recent colors | 3×2 | rectangular, round |
| [`night-light`](controls.md#night-light) | Controls | Warmer screen colors for the evening | 3×1 | rectangular, round |
<!-- ANCHOR_END: index -->

<!-- ANCHOR: clock -->
<div class="widget-facts">

| Name in `config.kdl` | `clock` |
|---|---|
| Title | Clock |
| Shows | Digital, or analog when round |
| Group | Time |
| Size in cells | 4×2 by default, 2×1 at least, 6×3 at most |
| Shapes | rectangular, round |

</div>

| Setting | Label in the sidebar | Values | Default |
|---|---|---|---|
| `hours` | Time format | `12`, `24` | `24` (`12` with a US locale) |
| `seconds` | Seconds | `#true`, `#false` | `#true` |
| `date` | Date | `#true`, `#false` | `#true` |
| `zones` | Other time zones | text (the sidebar's hint: *e.g. Asia/Tokyo, New York*) | empty |
<!-- ANCHOR_END: clock -->

<!-- ANCHOR: calendar -->
<div class="widget-facts">

| Name in `config.kdl` | `calendar` |
|---|---|
| Title | Calendar |
| Shows | Month view |
| Group | Time |
| Size in cells | 4×4 by default, 3×3 at least, 6×6 at most |
| Shapes | rectangular, round |

</div>

| Setting | Label in the sidebar | Values | Default |
|---|---|---|---|
| `first-day` | Week starts on | `"monday"`, `"sunday"` | `"monday"` (`"sunday"` with a US locale) |
| `week-numbers` | Week numbers | `#true`, `#false` | `#false` |
<!-- ANCHOR_END: calendar -->

<!-- ANCHOR: system-monitor -->
<div class="widget-facts">

| Name in `config.kdl` | `system-monitor` |
|---|---|
| Title | System monitor |
| Shows | CPU, memory, network, temperature |
| Group | System |
| Size in cells | 4×3 by default, 2×2 at least, 6×5 at most |
| Shapes | rectangular, round |

</div>

| Setting | Label in the sidebar | Values | Default |
|---|---|---|---|
| `history` | Chart covers | `30`, `60`, `90`, `120`, `180` (seconds) | `90` |
| `cores` | Per-core bars | `#true`, `#false` | `#true` |
<!-- ANCHOR_END: system-monitor -->

<!-- ANCHOR: windows -->
<div class="widget-facts">

| Name in `config.kdl` | `windows` |
|---|---|
| Title | Windows |
| Shows | Open windows by workspace; click to focus |
| Group | Activity |
| Size in cells | 5×5 by default, 3×2 at least, 8×8 at most |
| Shapes | rectangular |
| Settings | none |

</div>
<!-- ANCHOR_END: windows -->

<!-- ANCHOR: claude-sessions -->
<div class="widget-facts">

| Name in `config.kdl` | `claude-sessions` |
|---|---|
| Title | Claude sessions |
| Shows | Running Claude Code sessions; click to focus |
| Group | Activity |
| Size in cells | 4×3 by default, 3×2 at least, 6×5 at most |
| Shapes | rectangular, round |
| Settings | none |

</div>
<!-- ANCHOR_END: claude-sessions -->

<!-- ANCHOR: wifi -->
<div class="widget-facts">

| Name in `config.kdl` | `wifi` |
|---|---|
| Title | Wi-Fi |
| Shows | Networks, connect, radio on/off |
| Group | Devices |
| Size in cells | 3×4 by default, 2×2 at least, 4×6 at most |
| Shapes | rectangular, round |
| Settings | none |

</div>
<!-- ANCHOR_END: wifi -->

<!-- ANCHOR: bluetooth -->
<div class="widget-facts">

| Name in `config.kdl` | `bluetooth` |
|---|---|
| Title | Bluetooth |
| Shows | Devices, connect, adapter on/off |
| Group | Devices |
| Size in cells | 3×3 by default, 2×2 at least, 4×5 at most |
| Shapes | rectangular, round |
| Settings | none |

</div>
<!-- ANCHOR_END: bluetooth -->

<!-- ANCHOR: battery -->
<div class="widget-facts">

| Name in `config.kdl` | `battery` |
|---|---|
| Title | Battery & power |
| Shows | Charge, power profile; device batteries and sleep blockers when bigger |
| Group | System |
| Size in cells | 3×2 by default, 2×1 at least, 4×3 at most |
| Shapes | rectangular, round |
| Settings | none |

</div>
<!-- ANCHOR_END: battery -->

<!-- ANCHOR: volume -->
<div class="widget-facts">

| Name in `config.kdl` | `volume` |
|---|---|
| Title | Volume |
| Shows | Speaker and microphone; outputs and apps when bigger |
| Group | Controls |
| Size in cells | 3×3 by default, 2×1 at least, 4×4 at most |
| Shapes | rectangular, round |
| Settings | none |

</div>
<!-- ANCHOR_END: volume -->

<!-- ANCHOR: brightness -->
<div class="widget-facts">

| Name in `config.kdl` | `brightness` |
|---|---|
| Title | Brightness |
| Shows | Screen backlight |
| Group | Controls |
| Size in cells | 3×1 by default, 2×1 at least, 4×2 at most |
| Shapes | rectangular, round |
| Settings | none |

</div>
<!-- ANCHOR_END: brightness -->

<!-- ANCHOR: notifications -->
<div class="widget-facts">

| Name in `config.kdl` | `notifications` |
|---|---|
| Title | Notifications |
| Shows | Notification history |
| Group | Activity |
| Size in cells | 4×5 by default, 3×2 at least, 6×10 at most |
| Shapes | rectangular |
| Settings | none |

</div>
<!-- ANCHOR_END: notifications -->

<!-- ANCHOR: tray -->
<div class="widget-facts">

| Name in `config.kdl` | `tray` |
|---|---|
| Title | Tray |
| Shows | Status icons and their menus |
| Group | Controls |
| Size in cells | 4×1 by default, 1×1 at least, 8×2 at most |
| Shapes | rectangular |
| Settings | none |

</div>
<!-- ANCHOR_END: tray -->

<!-- ANCHOR: media -->
<div class="widget-facts">

| Name in `config.kdl` | `media` |
|---|---|
| Title | Media player |
| Shows | Now playing (MPRIS) |
| Group | Media |
| Size in cells | 5×2 by default, 3×1 at least, 6×3 at most |
| Shapes | rectangular |
| Settings | none |

</div>
<!-- ANCHOR_END: media -->

<!-- ANCHOR: session -->
<div class="widget-facts">

| Name in `config.kdl` | `session` |
|---|---|
| Title | Session |
| Shows | Lock, suspend, log out, power off |
| Group | Controls |
| Size in cells | 4×1 by default, 3×1 at least, 6×2 at most |
| Shapes | rectangular |

</div>

| Setting | Label in the sidebar | Values | Default |
|---|---|---|---|
| `lock` | Lock | `#true`, `#false` | `#true` |
| `suspend` | Suspend | `#true`, `#false` | `#true` |
| `logout` | Log out | `#true`, `#false` | `#true` |
| `restart` | Restart | `#true`, `#false` | `#true` |
| `power-off` | Power off | `#true`, `#false` | `#true` |
| `confirm` | Confirm restart and power off | `#true`, `#false` | `#true` |
<!-- ANCHOR_END: session -->

<!-- ANCHOR: clipboard -->
<div class="widget-facts">

| Name in `config.kdl` | `clipboard` |
|---|---|
| Title | Clipboard history |
| Shows | Records what you copy while it's on the dashboard, in memory only |
| Group | Activity |
| Size in cells | 4×4 by default, 3×2 at least, 6×8 at most |
| Shapes | rectangular |
| Settings | none |

</div>
<!-- ANCHOR_END: clipboard -->

<!-- ANCHOR: weather -->
<div class="widget-facts">

| Name in `config.kdl` | `weather` |
|---|---|
| Title | Weather |
| Shows | Now and the next days (Open-Meteo) |
| Group | Time |
| Size in cells | 4×2 by default, 2×1 at least, 6×4 at most |
| Shapes | rectangular, round |

</div>

| Setting | Label in the sidebar | Values | Default |
|---|---|---|---|
| `location` | Location | text (the sidebar's hint: *City of your time zone*) | empty |
| `units` | Units | `"celsius"`, `"fahrenheit"` | `"celsius"` (`"fahrenheit"` with a US locale) |
<!-- ANCHOR_END: weather -->

<!-- ANCHOR: memory -->
<div class="widget-facts">

| Name in `config.kdl` | `memory` |
|---|---|
| Title | Memory |
| Shows | RAM and swap, history, biggest apps |
| Group | System |
| Size in cells | 4×3 by default, 2×2 at least, 5×4 at most |
| Shapes | rectangular, round |

</div>

| Setting | Label in the sidebar | Values | Default |
|---|---|---|---|
| `history` | Chart covers | `30`, `60`, `90`, `120`, `180` (seconds) | `90` |
| `top-apps` | Top apps | `0` (none), `1`, `2`, `3`, `4`, `5`, `6`, `8`, `10` | `5` |
<!-- ANCHOR_END: memory -->

<!-- ANCHOR: disks -->
<div class="widget-facts">

| Name in `config.kdl` | `disks` |
|---|---|
| Title | Disks |
| Shows | Space per filesystem, read and write |
| Group | System |
| Size in cells | 4×2 by default, 3×1 at least, 6×3 at most |
| Shapes | rectangular, round |

</div>

| Setting | Label in the sidebar | Values | Default |
|---|---|---|---|
| `hide-system` | Hide /boot and /efi | `#true`, `#false` | `#false` |
<!-- ANCHOR_END: disks -->

<!-- ANCHOR: network -->
<div class="widget-facts">

| Name in `config.kdl` | `network` |
|---|---|
| Title | Network |
| Shows | Download and upload, history |
| Group | System |
| Size in cells | 4×2 by default, 2×1 at least, 5×3 at most |
| Shapes | rectangular, round |

</div>

| Setting | Label in the sidebar | Values | Default |
|---|---|---|---|
| `history` | Chart covers | `30`, `60`, `90`, `120`, `180` (seconds) | `90` |
<!-- ANCHOR_END: network -->

<!-- ANCHOR: gpu -->
<div class="widget-facts">

| Name in `config.kdl` | `gpu` |
|---|---|
| Title | GPU |
| Shows | GPU load, video memory, temperature |
| Group | System |
| Size in cells | 4×2 by default, 2×1 at least, 5×3 at most |
| Shapes | rectangular, round |

</div>

| Setting | Label in the sidebar | Values | Default |
|---|---|---|---|
| `history` | Chart covers | `30`, `60`, `90`, `120`, `180` (seconds) | `90` |
<!-- ANCHOR_END: gpu -->

<!-- ANCHOR: claude-usage -->
<div class="widget-facts">

| Name in `config.kdl` | `claude-usage` |
|---|---|
| Title | Claude usage |
| Shows | Plan, 5-hour window, tokens today |
| Group | Activity |
| Size in cells | 4×3 by default, 3×2 at least, 5×4 at most |
| Shapes | rectangular, round |
| Settings | none |

</div>
<!-- ANCHOR_END: claude-usage -->

<!-- ANCHOR: mascot -->
<div class="widget-facts">

| Name in `config.kdl` | `mascot` |
|---|---|
| Title | Mascot |
| Shows | Schraubi, Zolli or Verschnitti; says what you tell it |
| Group | Activity |
| Size in cells | 4×2 by default, 3×2 at least, 5×4 at most |
| Shapes | rectangular |

</div>

| Setting | Label in the sidebar | Values | Default |
|---|---|---|---|
| `character` | Mascot | `"schraubi"`, `"zolli"`, `"verschnitti"`, `"cow"` | `"schraubi"` |
| `wildness` | Wildness | `0` (calm), `1` (dry), `2` (cheeky), `3` (wild), `4` (unhinged) | `2` (cheeky) |
| `interval` | New remark every | `15`, `30`, `45`, `60`, `120`, `300`, `600` (seconds) | `45` |
| `command` | Lines from command | text (the sidebar's hint: *Built-in remarks*) | empty |
| `cowfile` | Cow file | text (the sidebar's hint: *e.g. tux, or a path (the cow only)*) | empty |
<!-- ANCHOR_END: mascot -->

<!-- ANCHOR: keyboard-layout -->
<div class="widget-facts">

| Name in `config.kdl` | `keyboard-layout` |
|---|---|
| Title | Keyboard layout |
| Shows | Current layout; click to switch, bigger shows all |
| Group | Controls |
| Size in cells | 2×1 by default, 1×1 at least, 3×1 at most |
| Shapes | rectangular, round |
| Settings | none |

</div>
<!-- ANCHOR_END: keyboard-layout -->

<!-- ANCHOR: color-picker -->
<div class="widget-facts">

| Name in `config.kdl` | `color-picker` |
|---|---|
| Title | Color picker |
| Shows | Pick a pixel anywhere; hex copied, recent colors |
| Group | Controls |
| Size in cells | 3×2 by default, 2×1 at least, 4×3 at most |
| Shapes | rectangular, round |
| Settings | none |

</div>
<!-- ANCHOR_END: color-picker -->

<!-- ANCHOR: timer -->
<div class="widget-facts">

| Name in `config.kdl` | `timer` |
|---|---|
| Title | Timer |
| Shows | Countdown with a ring; scroll to set, presets when bigger |
| Group | Time |
| Size in cells | 3×2 by default, 2×1 at least, 4×3 at most |
| Shapes | round, rectangular (a new one starts round) |
| Settings | none |

</div>
<!-- ANCHOR_END: timer -->

<!-- ANCHOR: stopwatch -->
<div class="widget-facts">

| Name in `config.kdl` | `stopwatch` |
|---|---|
| Title | Stopwatch |
| Shows | Laps; a seconds dial when round, the lap list when taller |
| Group | Time |
| Size in cells | 3×3 by default, 2×1 at least, 4×4 at most |
| Shapes | round, rectangular (a new one starts round) |
| Settings | none |

</div>
<!-- ANCHOR_END: stopwatch -->

<!-- ANCHOR: notes -->
<div class="widget-facts">

| Name in `config.kdl` | `notes` |
|---|---|
| Title | Sticky note |
| Shows | A note that keeps what you type; name it to have several |
| Group | Activity |
| Size in cells | 3×3 by default, 2×1 at least, 6×6 at most |
| Shapes | rectangular |

</div>

| Setting | Label in the sidebar | Values | Default |
|---|---|---|---|
| `note` | Note | text (the sidebar's hint: *notes (same name, same note)*) | empty |
<!-- ANCHOR_END: notes -->

<!-- ANCHOR: privacy -->
<div class="widget-facts">

| Name in `config.kdl` | `privacy` |
|---|---|
| Title | Privacy |
| Shows | Who uses the microphone, camera or screen; stop screen sharing |
| Group | System |
| Size in cells | 3×2 by default, 2×1 at least, 4×3 at most |
| Shapes | rectangular, round |
| Settings | none |

</div>
<!-- ANCHOR_END: privacy -->

<!-- ANCHOR: night-light -->
<div class="widget-facts">

| Name in `config.kdl` | `night-light` |
|---|---|
| Title | Night light |
| Shows | Warmer screen colors for the evening |
| Group | Controls |
| Size in cells | 3×1 by default, 2×1 at least, 4×2 at most |
| Shapes | rectangular, round |
| Settings | none |

</div>
<!-- ANCHOR_END: night-light -->

<!-- ANCHOR: removable-drives -->
<div class="widget-facts">

| Name in `config.kdl` | `removable-drives` |
|---|---|
| Title | Removable drives |
| Shows | USB drives and SD cards: mount, open, eject |
| Group | Devices |
| Size in cells | 3×2 by default, 2×1 at least, 4×4 at most |
| Shapes | rectangular |
| Settings | none |

</div>
<!-- ANCHOR_END: removable-drives -->

<!-- ANCHOR: phone -->
<div class="widget-facts">

| Name in `config.kdl` | `phone` |
|---|---|
| Title | Phone |
| Shows | KDE Connect: battery, ring to find it, send files |
| Group | Devices |
| Size in cells | 3×2 by default, 2×1 at least, 4×4 at most |
| Shapes | rectangular, round |
| Settings | none |

</div>
<!-- ANCHOR_END: phone -->

<!-- ANCHOR: sleep-timer -->
<div class="widget-facts">

| Name in `config.kdl` | `sleep-timer` |
|---|---|
| Title | Sleep timer |
| Shows | Count down to suspend, hibernate or power off (like sleepy) |
| Group | Time |
| Size in cells | 4×3 by default, 3×2 at least, 5×4 at most |
| Shapes | rectangular, round |

</div>

| Setting | Label in the sidebar | Values | Default |
|---|---|---|---|
| `sleep-when` | Starts with | text (the sidebar's hint: *30 min (or 45s, 1h30m, 23:45)*) | empty |
| `sleep-script` | Script to run | text (the sidebar's hint: *sleep.sh next to config.kdl*) | empty |
| `dry-run` | Dry run | `#true`, `#false` | `#false` |
| `keep-awake` | Keep the screen on | `#true`, `#false` | `#false` |
<!-- ANCHOR_END: sleep-timer -->

<!-- ANCHOR: pomodoro -->
<div class="widget-facts">

| Name in `config.kdl` | `pomodoro` |
|---|---|
| Title | Pomodoro |
| Shows | Focus sessions and breaks; a long break every few sessions |
| Group | Time |
| Size in cells | 3×2 by default, 2×1 at least, 4×3 at most |
| Shapes | round, rectangular (a new one starts round) |

</div>

| Setting | Label in the sidebar | Values | Default |
|---|---|---|---|
| `focus-minutes` | Focus session | `10`, `15`, `20`, `25`, `30`, `40`, `45`, `50`, `60`, `90` (minutes) | `25` |
| `short-break` | Short break | `3`, `5`, `7`, `10`, `15` (minutes) | `5` |
| `long-break` | Long break | `10`, `15`, `20`, `25`, `30`, `45` (minutes) | `15` |
| `rounds` | Sessions before the long break | `2`, `3`, `4`, `5`, `6`, `8` | `4` |
| `auto-start` | Start the next part by itself | `#true`, `#false` | `#false` |
<!-- ANCHOR_END: pomodoro -->

<!-- ANCHOR: about -->
<div class="widget-facts">

| Name in `config.kdl` | `about` |
|---|---|
| Title | About |
| Shows | Which version this is, its license, and what it is made with |
| Group | Activity |
| Size in cells | 2×2 by default, 2×2 at least, 4×4 at most |
| Shapes | rectangular |
| Settings | none |

</div>
<!-- ANCHOR_END: about -->

<!-- ANCHOR: niri-dash-info -->
<div class="widget-facts">

| Name in `config.kdl` | `niri-dash-info` |
|---|---|
| Title | niri-dash info |
| Shows | Version, source, commit and build: what this niri-dash is made of |
| Group | Activity |
| Size in cells | 4×4 by default, 3×3 at least, 6×6 at most |
| Shapes | rectangular |
| Settings | none |

</div>
<!-- ANCHOR_END: niri-dash-info -->

