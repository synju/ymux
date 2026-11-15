import QtQuick
import Quickshell
import qs.Commons as Commons
import qs.Commons
import "../Ymux.js" as Ymux

// Tree picker (spec 5.1, 5.6): monitors > sessions > workspaces > tiles. j/k or
// arrows move, h/l collapse and expand, Enter switches, x asks to kill a session or
// workspace (y confirms, any other key cancels), Escape closes.
Rectangle {
  id: card

  property bool active: false
  property var tree: []
  property string exe: ""
  // folded rows as the daemon remembers them (state.json), loaded on open
  property var folded: []
  property var collapsed: ({})
  property int index: 0
  property var confirmRow: null
  readonly property var rows: Ymux.treeRows(card.tree, card.collapsed)
  signal done()
  // `r`: rename this row; the menu swaps to the rename prompt
  signal renameRow(var payload)

  // fixed row height, so the card size follows the row count at once when folding
  // (sizing from the list's measured content lagged and clipped the last row)
  readonly property real rowHeight: Math.round(Style.font.body * 1.9)
  width: 560
  height: Math.min(560, card.rows.length * card.rowHeight + header.height + footer.height + 48)

  onIndexChanged: list.positionViewAtIndex(card.index, ListView.Contain)
  onRowsChanged: {
    if (card.index >= card.rows.length) card.index = Math.max(0, card.rows.length - 1)
    Qt.callLater(function() { list.positionViewAtIndex(card.index, ListView.Contain) })
  }
  radius: 12
  color: Commons.Color.menu.background
  border.color: Commons.Color.menu.border
  border.width: 1

  function run(args) {
    Quickshell.execDetached([card.exe].concat(args))
  }

  onActiveChanged: {
    if (card.active) {
      card.collapsed = Ymux.collapsedFrom(card.folded)
      card.confirmRow = null
      card.index = Ymux.preselect(card.rows)
      keys.forceActiveFocus()
    }
  }

  function toggle(row, open) {
    var c = Object.assign({}, card.collapsed)
    if (open) delete c[row.key]
    else c[row.key] = true
    card.collapsed = c
    card.run(["fold", row.key, open ? "off" : "on"])
  }

  MouseArea { anchors.fill: parent }

  Item {
    id: keys
    anchors.fill: parent
    focus: true
    Keys.onPressed: function(event) {
      var row = card.rows[card.index]
      if (card.confirmRow) {
        if (event.text === "y") {
          var cmd = Ymux.killCommand(card.confirmRow)
          if (cmd) card.run(cmd)
          card.done()
        } else {
          card.confirmRow = null
        }
        event.accepted = true
        return
      }
      if (event.key === Qt.Key_Escape) card.done()
      else if (event.key === Qt.Key_Down || event.text === "j") card.index = Math.min(card.index + 1, card.rows.length - 1)
      else if (event.key === Qt.Key_Up || event.text === "k") card.index = Math.max(card.index - 1, 0)
      else if ((event.key === Qt.Key_Left || event.text === "h") && row) card.toggle(row, false)
      else if ((event.key === Qt.Key_Right || event.text === "l") && row) card.toggle(row, true)
      else if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter) && row) {
        card.run(Ymux.pickCommand(row))
        card.done()
      } else if (event.text === "x" && row && Ymux.killCommand(row)) card.confirmRow = row
      else if (event.text === "r" && row) card.renameRow(Ymux.renamePayload(row))
      event.accepted = true
    }
  }

  Text {
    id: header
    anchors.top: parent.top
    anchors.left: parent.left
    anchors.margins: 16
    text: card.confirmRow ? "Kill " + card.confirmRow.label + "? (y/n)" : "ymux"
    color: card.confirmRow ? Commons.Color.urgent : Commons.Color.menu.text
    font.family: Style.font.family
    font.pixelSize: Style.font.body
    font.bold: true
  }

  ListView {
    id: list
    anchors.top: header.bottom
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.bottom: footer.top
    anchors.margins: 12
    clip: true
    model: card.rows
    currentIndex: card.index
    delegate: Rectangle {
      required property var modelData
      required property int index
      width: list.width
      height: card.rowHeight
      radius: 6
      color: index === card.index ? Commons.Color.menu.selectedBackground : "transparent"
      Text {
        id: label
        anchors.verticalCenter: parent.verticalCenter
        x: 8 + modelData.depth * 18
        text: (card.collapsed[modelData.key] ? "+ " : "") + modelData.label
        color: index === card.index ? Commons.Color.menu.selectedText : Commons.Color.menu.text
        font.family: Style.font.family
        font.pixelSize: Style.font.body
        font.bold: modelData.current
      }
    }
  }

  Text {
    id: footer
    anchors.bottom: parent.bottom
    anchors.left: parent.left
    anchors.margins: 12
    text: "Enter switch   r rename   x kill   h/l fold   Esc close"
    color: Commons.Color.muted
    font.family: Style.font.family
    font.pixelSize: Style.font.body * 0.85
  }
}
