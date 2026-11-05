# How to run a script when the sleep timer fires

The sleep timer can run a script of yours instead of suspending, hibernating or powering off. The
script mode is offered once the script file exists.

1. Write the script. By default it is `~/.config/niri-dash/sleep.sh`:

   ```sh
   #!/bin/sh
   # Pause the music, then put the machine to sleep.
   playerctl pause
   systemctl suspend
   ```

   If the file is executable it runs as it is. If it isn't, it runs through `sh`. It gets no
   arguments.

2. Use a different file if you want. Set the sleep timer widget's `sleep-script` setting, in its
   settings card or in `config.kdl`; a leading `~/` is your home directory:

   ```kdl
   layout {
       sleep-timer col=0 row=0 width=4 height=3 {
           sleep-script "~/bin/goodnight.sh"
       }
   }
   ```

3. In the widget, pick the **run script** mode with `j`/`k` (or the arrows) and arm the timer.

## Test it without waiting

Switch on **Dry run** (`d` or Ctrl+D inside the widget). The countdown runs for real, and at the
end the widget only says what would have run, which is the script's path.

For the widget's keys and settings, see [Time widgets](../reference/widgets/time.md#sleep-timer).
