# Motion setup and validation

Tyler leaves native desktop transitions to macOS. Window rearrangements now use
immediate shortcut wakeups, Accessibility notifications and batched destinations.
Apps that omit events are reconciled once per second. Impossible sizes are learned
after stable readback; AX timeouts are not treated as minimum window sizes.

## Try the animation prototype

Add or update this section in `~/.config/tyler/tyler.toml`, or the file selected
by `TYLER_CONFIG`. Saving hot-reloads it:

```toml
[animations]
enabled = true
duration_ms = 160
```

The default is `enabled = false` so the responsiveness/constraint improvements can
be evaluated independently. `duration_ms` accepts 80 through 400 milliseconds.
Set `enabled = false` to compare immediate tiling with animation. This setting
does not change Mission Control's own animation. macOS Reduce Motion overrides
the prototype.

Animation uses the main display's CoreVideo clock, with a time-based ease-out
curve. Missed ticks skip ahead; they never queue obsolete frames. Repeated commands
retarget from the last submitted rectangle. Windows are validated at their final
size, and animation stops on desktop changes, close/minimize, backend failure,
display changes and quit. Deferred destinations are applied only on the active
native desktop. There is no idle display-link activity.

If three AX frame batches take more than 24 ms during the session, Tyler logs
the problem and switches to immediate tiling. Reload the config to retry after
resolving an unresponsive app. There is no Screen Recording or SIP change for this
prototype.

## Check and compare

From the checkout, after building:

```sh
./target/release/tyler-tiler check-config
./target/release/tyler-tiler check-motion
```

`check-motion` probes display-clock delivery to the main loop for 500 ms without
moving any windows. It reports the tick count, average/p95 interval and Reduce
Motion state. It does **not** measure AX throughput or visible presentation FPS.

Use disposable windows in Terminal, Chrome and the apps you use most:

1. Open/close windows and change splits. Look for one final arrangement without
   a series of resize/retry jumps. Compare with animation disabled first.
2. Enable animation, swap windows, resize, maximize and restore. Repeat shortcuts
   rapidly; motion should follow the latest destination without queued playback.
3. Switch native desktops with both shortcuts and trackpad gestures. No old
   tween should continue on another desktop. The settling delay is a heuristic;
   macOS exposes no public Mission Control-animation completion callback here.
4. Enable Reduce Motion and verify immediate tiling. Quit during motion and check
   that no window continues moving and scratchpad windows are restored.
5. Check `~/Library/Logs/Tyler.log` for submitted batch timing and automatic
   fallback. These timings measure AX work, not what the display presented.

The acceptance gate is consistent visible motion at the display's refresh rate,
without input lag or corrective jumps in the everyday apps above. This requires
an on-screen check and frame-time capture; passing unit tests or the clock probe
does not establish it. If AX animation cannot pass, evaluate a different backend
before adding more animation effects. Yabai's documented animation backend needs
Screen Recording permission and partially disabled SIP; it is not enabled by this
prototype.
