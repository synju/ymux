import QtQuick
import QtTest
import "../Ymux.js" as Ymux

TestCase {
  name: "Ymux"

  function view() {
    return { mode: "", marked: false, monitors: [{ connector: "DP-1", session_label: "main",
      workspaces: [{ id: 100001, number: 1, label: "foot 1", current: true, empty: false }] }] }
  }

  function test_parseView_rejects_garbage() { compare(Ymux.parseView("{oops"), null) }

  function test_parseView_accepts_view() { compare(Ymux.parseView(JSON.stringify(view())).monitors.length, 1) }

  function test_barRow_for_connector() {
    var r = Ymux.barRow(view(), "DP-1", false)
    compare(r.session, "main")
    compare(r.items[0].text, "foot 1")
    compare(r.items[0].current, true)
    compare(r.items[0].id, 100001)
  }

  function test_unknown_connector_gives_null_row() {
    compare(Ymux.monitorRow({ monitors: [] }, "DP-9"), null)
    compare(Ymux.barRow({ monitors: [] }, "DP-9", false), null)
  }

  function test_markers() {
    var v = view(); v.mode = "LEADER"; v.marked = true
    compare(Ymux.barRow(v, "DP-1", false).markers, ["LEADER", "MARK"])
    v.mode = ""; v.marked = false
    compare(Ymux.barRow(v, "DP-1", false).markers, [])
  }

  function test_foreign_shape_gives_null_row() {
    // the phase 2 daemon streamed raw state: monitors without workspaces
    compare(Ymux.barRow({ monitors: [{ connector: "DP-1", key: "LG A" }] }, "DP-1", false), null)
  }

  function test_divider_between_session_and_workspaces() {
    compare(Ymux.barRow(view(), "DP-1", false).divider, "||")
    compare(Ymux.barRow(view(), "DP-1", true).divider, "")
  }

  function test_items_split_name_and_number() {
    var v = view()
    v.monitors[0].workspaces.push({ id: 100002, number: 2, label: "2", current: false, empty: true })
    var r = Ymux.barRow(v, "DP-1", false)
    compare(r.items[0].name, "foot")
    compare(r.items[0].number, 1)
    compare(r.items[1].name, "")
    compare(r.items[1].number, 2)
  }

  function test_vertical_numbers_only() {
    var r = Ymux.barRow(view(), "DP-1", true)
    compare(r.items[0].text, "1")
    compare(r.session, "")
  }

  function test_bad_payload_is_empty_view() {
    compare(Ymux.parsePayload("nope").view, "")
    compare(Ymux.parsePayload('{"view":"rename","target":"workspace","id":5,"current":"x"}').view, "rename")
  }

  function test_prompt_commands() {
    compare(Ymux.promptCommand({ view: "rename", target: "workspace", id: 100002 }, "notes"), ["rename-workspace", "100002", "notes"])
    compare(Ymux.promptCommand({ view: "rename", target: "session", id: 3 }, "Work"), ["rename-session", "3", "Work"])
    compare(Ymux.promptCommand({ view: "rename", target: "workspace", id: 7 }, ""), ["rename-workspace", "7"])
  }

  function test_prompt_titles() {
    compare(Ymux.promptTitle({ view: "rename", target: "workspace" }), "Rename workspace")
    compare(Ymux.promptTitle({ view: "rename", target: "session" }), "Rename session")
  }

  function test_renumber_prompt() {
    compare(Ymux.promptCommand({ view: "renumber", id: 100001 }, "5"), ["renumber-workspace", "100001", "5"])
    compare(Ymux.promptTitle({ view: "renumber" }), "Workspace number")
    compare(Ymux.isNumeric({ view: "renumber" }), true)
    compare(Ymux.isNumeric({ view: "rename", target: "workspace" }), false)
  }

  function test_overlay_views_that_open() {
    compare(Ymux.opensPrompt({ view: "rename" }), true)
    compare(Ymux.opensPrompt({ view: "renumber" }), true)
    compare(Ymux.opensPrompt({ view: "" }), false)
  }

  function tree() {
    return [{ connector: "DP-1", description: "LG A", focused: true, sessions: [
      { number: 1, label: "main", active: true, workspaces: [
        { id: 100001, number: 1, label: "1", shown: true, tiles: [{ address: "0xa1", class: "foot", title: "t", focused: true }] }] }] }]
  }

  function test_tree_rows_flatten_in_order() {
    var rows = Ymux.treeRows(tree(), {})
    compare(rows.map(function(r) { return r.kind }), ["monitor", "session", "workspace", "tile"])
    compare(rows.map(function(r) { return r.depth }), [0, 1, 2, 3])
  }

  function test_collapsed_session_hides_children() {
    compare(Ymux.treeRows(tree(), { "s:1": true }).length, 2)
  }

  function test_preselect_deepest_current() {
    compare(Ymux.preselect(Ymux.treeRows(tree(), {})), 3)
  }

  function test_pick_commands() {
    var rows = Ymux.treeRows(tree(), {})
    compare(Ymux.pickCommand(rows[0]), ["focus-monitor", "DP-1"])
    compare(Ymux.pickCommand(rows[1]), ["switch", "1"])
    compare(Ymux.pickCommand(rows[2]), ["focus-id", "100001"])
    compare(Ymux.pickCommand(rows[3]), ["focus-window", "0xa1"])
  }

  function test_kill_commands() {
    var rows = Ymux.treeRows(tree(), {})
    compare(Ymux.killCommand(rows[1]), ["kill-session", "1"])
    compare(Ymux.killCommand(rows[2]), ["kill-workspace", "100001"])
    compare(Ymux.killCommand(rows[3]), null)
  }

  function test_command_prompt() {
    compare(Ymux.promptCommand({ view: "command" }, "new Work"), ["command", "new Work"])
    compare(Ymux.opensPrompt({ view: "command" }), true)
    compare(Ymux.promptTitle({ view: "command" }), "Command")
  }

  function test_pane_for_key() {
    var p = [{ key: "1", address: "0x1" }, { key: "a", address: "0xa" }]
    compare(Ymux.paneForKey(p, "1").address, "0x1")
    compare(Ymux.paneForKey(p, "A").address, "0xa")
    compare(Ymux.paneForKey(p, "z"), null)
  }

  function test_collapsed_from_folded_keys() {
    var c = Ymux.collapsedFrom(["s:1", "w:2"])
    compare(c["s:1"], true)
    compare(c["w:2"], true)
    compare(c["s:3"], undefined)
    compare(Object.keys(Ymux.collapsedFrom(null)).length, 0)
  }

  function test_rename_payload_for_every_row_kind() {
    compare(JSON.stringify(Ymux.renamePayload({ kind: "monitor", description: "LG A", name: "Main" })),
            JSON.stringify({ view: "rename", target: "monitor", id: "LG A", current: "Main" }))
    compare(Ymux.renamePayload({ kind: "session", id: 2, name: "work" }).target, "session")
    compare(Ymux.renamePayload({ kind: "workspace", id: 100001, name: "" }).current, "")
    compare(Ymux.renamePayload({ kind: "tile", address: "0xa", name: "" }).id, "0xa")
  }

  function test_prompt_commands_for_monitor_and_app() {
    compare(Ymux.promptCommand({ view: "rename", target: "monitor", id: "LG A" }, "Main"), ["rename-monitor", "LG A", "Main"])
    compare(Ymux.promptCommand({ view: "rename", target: "tile", id: "0xa" }, ""), ["rename-tile", "0xa"])
    compare(Ymux.promptTitle({ view: "rename", target: "monitor" }), "Rename monitor")
    compare(Ymux.promptTitle({ view: "rename", target: "tile" }), "Rename app")
  }

  function test_tree_rows_show_given_names() {
    var tree = [{ connector: "DP-1", description: "LG A", name: "Main", focused: true, sessions: [
      { number: 1, label: "main", name: "main", active: true, workspaces: [
        { id: 100001, label: "1", name: null, shown: true, tiles: [
          { address: "0xa", class: "foot", title: "t", name: "editor", focused: false }] }] }] }]
    var rows = Ymux.treeRows(tree, {})
    compare(rows[0].label, "Main")
    compare(rows[0].description, "LG A")
    compare(rows[3].label, "editor")
    compare(rows[3].name, "editor")
  }
}
