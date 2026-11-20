import QtQuick
import Quickshell
import qs.Commons as Commons
import qs.Commons
import "../Ymux.js" as Ymux

// Leader + o (spec 5.4): an Options section of toggles and, kept apart, an Actions
// section for the current workspace and session. j/k or arrows move, Space or Enter
// toggles or runs, Escape closes. Closing a workspace or session that holds apps asks
// y/n first; moving a workspace picks a session from a list.
Rectangle {
  id: card

  property bool active: false
  property bool relaunch: true
  property bool keepSpots: false
  property bool autoClose: false
  property var view: null
  property string exe: ""
  property int index: 0
  // "main", "pick" (a session to move the workspace to) or "confirm" (y/n)
  property string mode: "main"
  property int pick: 0
  property var pending: null // { text, cmd } waiting for y
  signal done()
  // rename / renumber: the menu swaps to that prompt
  signal openPayload(var payload)

  readonly property var ctx: Ymux.actionContext(card.view)
  readonly property var toggles: [
    ["Relaunch apps on restart", card.relaunch, "set-relaunch"],
    ["Keep a moved or closed app's spot open", card.keepSpots, "set-keep-spots"],
    ["Close empty workspaces and sessions automatically", card.autoClose, "set-auto-close"]
  ]
  readonly property var actions: card.ctx ? [
    ["rename-ws", "Rename workspace " + card.ctx.workspace.label],
    ["renumber-ws", "Renumber workspace " + card.ctx.workspace.label],
    ["move-ws", "Move workspace " + card.ctx.workspace.label + " to another session"],
    ["close-ws", "Close workspace " + card.ctx.workspace.label],
    ["rename-session", "Rename session " + card.ctx.session.label],
    ["close-session", "Close session " + card.ctx.session.label]
  ] : []
  readonly property int count: card.toggles.length + card.actions.length

  width: 520
  height: column.implicitHeight + 32
  radius: 12
  color: Commons.Color.menu.background
  border.color: Commons.Color.menu.border
  border.width: 1

  onActiveChanged: if (card.active) {
    card.index = 0
    card.mode = "main"
    card.pending = null
    keys.forceActiveFocus()
  }

  function run(args) {
    Quickshell.execDetached([card.exe].concat(args))
    card.done()
  }

  function apps(n) {
    return n === 1 ? "1 app" : n + " apps"
  }

  function act(key) {
    var w = card.ctx.workspace, s = card.ctx.session
    if (key === "rename-ws") card.openPayload({ view: "rename", target: "workspace", id: w.id, current: w.name })
    else if (key === "renumber-ws") card.openPayload({ view: "renumber", id: w.id, current: String(w.number) })
    else if (key === "move-ws") {
      if (card.ctx.sessions.length > 0) { card.pick = 0; card.mode = "pick" }
    } else if (key === "close-ws") {
      var cw = ["kill-workspace", String(w.id)]
      if (w.apps > 0) { card.pending = { text: "Close workspace " + w.label + " and its " + card.apps(w.apps) + "? (y/n)", cmd: cw }; card.mode = "confirm" }
      else card.run(cw)
    } else if (key === "rename-session") card.openPayload({ view: "rename", target: "session", id: s.number, current: s.name })
    else if (key === "close-session") {
      var cs = ["kill-session", String(s.number)]
      if (s.apps > 0) { card.pending = { text: "Close session " + s.label + ", its " + s.workspaces + " workspaces and " + card.apps(s.apps) + "? (y/n)", cmd: cs }; card.mode = "confirm" }
      else card.run(cs)
    }
  }

  MouseArea { anchors.fill: parent }

  Item {
    id: keys
    anchors.fill: parent
    focus: true
    Keys.onPressed: function(event) {
      var down = event.key === Qt.Key_Down || event.text === "j"
      var up = event.key === Qt.Key_Up || event.text === "k"
      var enter = event.key === Qt.Key_Space || event.key === Qt.Key_Return || event.key === Qt.Key_Enter
      if (card.mode === "confirm") {
        if (event.text === "y") card.run(card.pending.cmd)
        else { card.mode = "main"; card.pending = null }
      } else if (card.mode === "pick") {
        if (event.key === Qt.Key_Escape) card.mode = "main"
        else if (down) card.pick = Math.min(card.pick + 1, card.ctx.sessions.length - 1)
        else if (up) card.pick = Math.max(card.pick - 1, 0)
        else if (enter) card.run(["move-workspace-to", String(card.ctx.workspace.id), String(card.ctx.sessions[card.pick].number)])
      } else if (event.key === Qt.Key_Escape) card.done()
      else if (down) card.index = Math.min(card.index + 1, card.count - 1)
      else if (up) card.index = Math.max(card.index - 1, 0)
      else if (enter) {
        if (card.index < card.toggles.length) {
          var t = card.toggles[card.index]
          Quickshell.execDetached([card.exe, t[2], t[1] ? "off" : "on"])
        } else card.act(card.actions[card.index - card.toggles.length][0])
      }
      event.accepted = true
    }
  }

  component OptionRow: Rectangle {
    property string label: ""
    property bool selected: false
    width: column.width
    height: rowText.implicitHeight + 12
    radius: 6
    color: selected ? Commons.Color.menu.selectedBackground : "transparent"
    Text {
      id: rowText
      anchors.verticalCenter: parent.verticalCenter
      x: 8
      width: parent.width - 16
      elide: Text.ElideRight
      text: parent.label
      color: parent.selected ? Commons.Color.menu.selectedText : Commons.Color.menu.text
      font.family: Style.font.family
      font.pixelSize: Style.font.body
    }
  }

  component Heading: Text {
    color: Commons.Color.accent
    font.family: Style.font.family
    font.pixelSize: Style.font.body * 0.9
    font.bold: true
  }

  Column {
    id: column
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.top: parent.top
    anchors.margins: 16
    spacing: 6

    Text {
      text: "ymux"
      color: Commons.Color.menu.text
      font.family: Style.font.family
      font.pixelSize: Style.font.body
      font.bold: true
    }

    // ---- main: options, then actions ----
    Heading { visible: card.mode === "main"; text: "Options" }
    Repeater {
      model: card.mode === "main" ? card.toggles : []
      OptionRow {
        required property var modelData
        required property int index
        label: (modelData[1] ? "[x]" : "[ ]") + "  " + modelData[0]
        selected: index === card.index
      }
    }
    Heading { visible: card.mode === "main" && card.actions.length > 0; topPadding: 8; text: "Actions" }
    Repeater {
      model: card.mode === "main" ? card.actions : []
      OptionRow {
        required property var modelData
        required property int index
        label: modelData[1]
        selected: index + card.toggles.length === card.index
      }
    }

    // ---- pick a session to move the workspace to ----
    Heading { visible: card.mode === "pick"; text: card.ctx ? "Move workspace " + card.ctx.workspace.label + " to" : "" }
    Repeater {
      model: card.mode === "pick" && card.ctx ? card.ctx.sessions : []
      OptionRow {
        required property var modelData
        required property int index
        label: modelData.label + "  (" + modelData.monitor + ")"
        selected: index === card.pick
      }
    }

    // ---- y/n before closing apps ----
    Text {
      visible: card.mode === "confirm"
      width: column.width
      wrapMode: Text.WordWrap
      text: card.pending ? card.pending.text : ""
      color: Commons.Color.urgent
      font.family: Style.font.family
      font.pixelSize: Style.font.body
    }

    Text {
      topPadding: 6
      text: card.mode === "confirm" ? "y to confirm, any other key to go back"
          : card.mode === "pick" ? "j/k to choose, Enter to move, Escape to go back"
          : "j/k to choose, Space or Enter to toggle or run, Escape to close"
      color: Commons.Color.muted
      font.family: Style.font.family
      font.pixelSize: Style.font.body * 0.85
    }
  }
}
