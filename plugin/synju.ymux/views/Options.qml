import QtQuick
import Quickshell
import qs.Commons as Commons
import qs.Commons

// Options modal (spec 5.4): one setting in v1. Space or Enter toggles; Escape closes.
Rectangle {
  id: card

  property bool active: false
  property bool relaunch: true
  property string exe: ""
  signal done()

  width: 420
  height: column.implicitHeight + 32
  radius: 12
  color: Commons.Color.menu.background
  border.color: Commons.Color.menu.border
  border.width: 1

  onActiveChanged: if (card.active) keys.forceActiveFocus()

  MouseArea { anchors.fill: parent }

  Item {
    id: keys
    anchors.fill: parent
    focus: true
    Keys.onPressed: function(event) {
      if (event.key === Qt.Key_Escape) card.done()
      else if (event.key === Qt.Key_Space || event.key === Qt.Key_Return || event.key === Qt.Key_Enter)
        Quickshell.execDetached([card.exe, "set-relaunch", card.relaunch ? "off" : "on"])
      event.accepted = true
    }
  }

  Column {
    id: column
    anchors.fill: parent
    anchors.margins: 16
    spacing: 10

    Text {
      text: "ymux options"
      color: Commons.Color.menu.text
      font.family: Style.font.family
      font.pixelSize: Style.font.body
      font.bold: true
    }

    Rectangle {
      width: parent.width
      height: row.implicitHeight + 12
      radius: 6
      color: Commons.Color.menu.selectedBackground
      Text {
        id: row
        anchors.verticalCenter: parent.verticalCenter
        x: 8
        text: (card.relaunch ? "[x]" : "[ ]") + "  Relaunch apps on restart"
        color: Commons.Color.menu.selectedText
        font.family: Style.font.family
        font.pixelSize: Style.font.body
      }
    }

    Text {
      text: "Space to toggle, Escape to close"
      color: Commons.Color.muted
      font.family: Style.font.family
      font.pixelSize: Style.font.body * 0.85
    }
  }
}
