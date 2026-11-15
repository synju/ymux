# ymux Core (phases 1-2) - Live Checklist

No git repo yet (Task 2 asks before git init). Ticked as work lands.
Spec: `docs/spec.md`
Plan: `docs/superpowers/plans/2026-11-14-ymux-core.md`

1. **Spikes S1-S4 (live desktop)**
   - [x] S3 daemon start from top-level Lua
   - [x] S1 reconcile off, socket2 event formats
   - [x] S2 submap, catch-all, unbind semantics
   - [x] S4 cross-workspace swap, float event
   - [x] Desktop reverted, verdicts recorded
   - [x] Done  ·  [ ] Reviewed
2. **Scaffold and model**
   - [x] Types, slot ranges, id allocation
   - [x] Built  ·  [ ] Reviewed
3. **Config file**
   - [x] Parser, defaults, bad-line handling
   - [x] Relaunch line rewrite
   - [x] Built  ·  [ ] Reviewed
4. **ymux.lua generator**
   - [x] Leader submap, binds, takeovers
   - [x] Reconcile off, daemon start last
   - [x] Built  ·  [ ] Reviewed
5. **Hyprland adapter**
   - [x] Hypr trait, hyprctl impl, Lua builders
   - [x] socket2 parser, FakeHypr, fixtures
   - [x] Built  ·  [ ] Reviewed
6. **Engine: workspace commands**
   - [x] Focus, move, next/prev, previous
   - [x] New workspace, new session, switch
   - [x] Built  ·  [ ] Reviewed
7. **Engine: events and monitors**
   - [x] Tile tracking, event-driven deletion
   - [x] Monitor invariant, session move
   - [x] Specials ignored, live adoption
   - [x] Built  ·  [ ] Reviewed
8. **Layout modes**
   - [x] Mode reader chain, mode-file mover
   - [x] Mode file deleted with workspace
   - [x] Built  ·  [ ] Reviewed
9. **Start kinds**
   - [x] Adoption, restart rules, landing
   - [x] Built  ·  [ ] Reviewed
10. **Store, IPC, CLI**
    - [x] Atomic state, socket protocol, CLI
    - [x] Built  ·  [ ] Reviewed
11. **Daemon loop**
    - [x] Lock wait, lifetime exits, watchers
    - [x] Config watch, reload only on change
    - [x] Outside-renumber check on tick
    - [x] Built  ·  [ ] Reviewed
12. **Install and uninstall**
    - [x] Install command, require line
    - [x] Uninstall script keeps only config
    - [x] Built  ·  [ ] Reviewed
13. **Live acceptance (user)**
    - [ ] Keys on both monitors
    - [ ] Reload, unplug/replug, daemon restart
    - [ ] Config edit, uninstall, reinstall

Close-out:
- [x] Final whole-branch adversarial review
- [ ] User acceptance (Task 13 all pass)
- [ ] Plans for phases 3-6 written
