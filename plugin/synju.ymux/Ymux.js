.pragma library

// Pure helpers over the `ymux watch` View JSON (src/view.rs).

function parseView(line) {
  try {
    var v = JSON.parse(line)
    return (v && typeof v === "object" && Array.isArray(v.monitors)) ? v : null
  } catch (e) {
    return null
  }
}

function monitorRow(view, connector) {
  if (!view || !Array.isArray(view.monitors)) return null
  for (var i = 0; i < view.monitors.length; i++) {
    if (view.monitors[i].connector === connector) return view.monitors[i]
  }
  return null
}

// One bar's content: session label, workspace segments, mode markers.
function barRow(view, connector, vertical) {
  var row = monitorRow(view, connector)
  if (!row || !Array.isArray(row.workspaces)) return null
  var items = []
  for (var i = 0; i < row.workspaces.length; i++) {
    var w = row.workspaces[i]
    var num = String(w.number)
    // the label is `name number` for a named workspace, else just the number
    var name = (w.label !== num && w.label.length > num.length + 1) ? w.label.slice(0, -(num.length + 1)) : ""
    items.push({ id: w.id, text: vertical ? num : w.label, name: name, number: w.number, current: w.current, empty: w.empty })
  }
  var markers = []
  if (view.mode) markers.push(view.mode)
  if (view.marked) markers.push("MARK")
  var session = vertical ? "" : (row.session_label || "")
  // a separator between the session name and its workspaces (user request)
  return { session: session, divider: session ? "||" : "", items: items, markers: markers }
}

// ---- overlays (spec 5) ----

function parsePayload(json) {
  try {
    var p = JSON.parse(json)
    return (p && typeof p === "object" && typeof p.view === "string") ? p : { view: "" }
  } catch (e) {
    return { view: "" }
  }
}

// CLI arguments for the text a prompt submits (rename now; command prompt later).
function promptCommand(payload, text) {
  if (payload.view === "renumber") return ["renumber-workspace", String(payload.id), text]
  if (payload.view === "rename") {
    var verbs = { session: "rename-session", workspace: "rename-workspace", monitor: "rename-monitor", tile: "rename-tile" }
    var cmd = [verbs[payload.target] || "rename-workspace", String(payload.id)]
    if (text !== "") cmd.push(text)
    return cmd
  }
  return ["command", text]
}

// Leader + `.` asks for a whole number of 1 or more.
function isNumeric(payload) {
  return payload.view === "renumber"
}

function opensPrompt(payload) {
  return payload.view === "rename" || payload.view === "renumber" || payload.view === "command"
}

function promptTitle(payload) {
  if (payload.view === "renumber") return "Workspace number"
  if (payload.view === "command") return "Command"
  if (payload.view === "rename") {
    var titles = { session: "Rename session", workspace: "Rename workspace", monitor: "Rename monitor", tile: "Rename app" }
    return titles[payload.target] || "Rename workspace"
  }
  return "ymux"
}

// ---- tree picker (spec 5.1) ----

// Flattened rows: monitor > session > workspace > tile, children of a collapsed
// key omitted. `current` marks the focused monitor, active session, shown workspace
// and focused tile.
// `r` in the tree: the rename prompt for any row (user request 2026-11-15).
function renamePayload(row) {
  var id = row.kind === "monitor" ? row.description : row.kind === "tile" ? row.address : row.id
  return { view: "rename", target: row.kind, id: id, current: row.name || "" }
}

// The daemon's folded keys as the tree's collapsed map.
function collapsedFrom(folded) {
  var c = {}
  if (!folded) return c
  for (var i = 0; i < folded.length; i++) c[folded[i]] = true
  return c
}

function treeRows(tree, collapsed) {
  var rows = []
  collapsed = collapsed || {}
  for (var i = 0; i < tree.length; i++) {
    var m = tree[i]
    var mk = "m:" + m.connector
    rows.push({ kind: "monitor", depth: 0, key: mk, label: m.name ? m.name : m.connector + "  " + m.description, name: m.name || "", connector: m.connector, description: m.description, current: m.focused })
    if (collapsed[mk]) continue
    for (var j = 0; j < m.sessions.length; j++) {
      var s = m.sessions[j]
      var sk = "s:" + s.number
      rows.push({ kind: "session", depth: 1, key: sk, label: s.label, name: s.name || "", id: s.number, current: m.focused && s.active })
      if (collapsed[sk]) continue
      for (var k = 0; k < s.workspaces.length; k++) {
        var w = s.workspaces[k]
        var wk = "w:" + w.id
        rows.push({ kind: "workspace", depth: 2, key: wk, label: w.label, name: w.name || "", id: w.id, current: m.focused && s.active && w.shown })
        if (collapsed[wk]) continue
        for (var t = 0; t < w.tiles.length; t++) {
          var tile = w.tiles[t]
          rows.push({ kind: "tile", depth: 3, key: "t:" + tile.address, label: tile.name ? tile.name : tile.class + "  " + tile.title, name: tile.name || "", address: tile.address, current: tile.focused })
        }
      }
    }
  }
  return rows
}

// The deepest current row, else the first.
function preselect(rows) {
  var best = 0, depth = -1
  for (var i = 0; i < rows.length; i++) {
    if (rows[i].current && rows[i].depth > depth) { best = i; depth = rows[i].depth }
  }
  return best
}

function pickCommand(row) {
  if (row.kind === "monitor") return ["focus-monitor", row.connector]
  if (row.kind === "session") return ["switch", String(row.id)]
  if (row.kind === "workspace") return ["focus-id", String(row.id)]
  return ["focus-window", row.address]
}

// `x` in the tree: sessions and workspaces only.
function killCommand(row) {
  if (row.kind === "session") return ["kill-session", String(row.id)]
  if (row.kind === "workspace") return ["kill-workspace", String(row.id)]
  return null
}

// ---- tile numbers (spec 5.5) ----

function paneForKey(panes, key) {
  var k = String(key).toLowerCase()
  for (var i = 0; i < panes.length; i++) {
    if (panes[i].key === k) return panes[i]
  }
  return null
}
