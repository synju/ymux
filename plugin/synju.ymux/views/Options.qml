import QtQuick
import Quickshell
import qs.Commons as Commons
import qs.Commons

// Options modal (spec 5.4): j/k or arrows choose a setting, Space or Enter toggles it,
// Escape closes.
Rectangle {
  id: card

  property bool active: false
  property bool relaunch: true
  property bool keepSpots: false
  property string exe: ""
  property int index: 0
  signal done()

  // [label, current value, command that sets it]
  readonly property var settings: [
    ["Relaunch apps on restart", card.relaunch, "set-relaunch"],
    ["Keep a moved or closed app's spot open", card.keepSpots, "set-keep-spots"]
  ]

  width: 460
  height: column.implicitHeight + 32
  radius: 12
  color: Commons.Color.menu.background
  border.color: Commons.Color.menu.border
  border.width: 1

  onActiveChanged: if (card.active) {
    card.index = 0
    keys.forceActiveFocus()
  }

  MouseArea { anchors.fill: parent }

  Item {
    id: keys
    anchors.fill: parent
    focus: true
    Keys.onPressed: function(event) {
      if (event.key === Qt.Key_Escape) card.done()
      else if (event.key === Qt.Key_Down || event.text === "j") card.index = Math.min(card.index + 1, card.settings.length - 1)
      else if (event.key === Qt.Key_Up || event.text === "k") card.index = Math.max(card.index - 1, 0)
      else if (event.key === Qt.Key_Space || event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
        var s = card.settings[card.index]
        Quickshell.execDetached([card.exe, s[2], s[1] ? "off" : "on"])
      }
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

    Repeater {
      model: card.settings

      Rectangle {
        required property var modelData
        required property int index
        readonly property bool selected: index === card.index
        width: column.width
        height: row.implicitHeight + 12
        radius: 6
        color: selected ? Commons.Color.menu.selectedBackground : "transparent"
        Text {
          id: row
          anchors.verticalCenter: parent.verticalCenter
          x: 8
          text: (modelData[1] ? "[x]" : "[ ]") + "  " + modelData[0]
          color: selected ? Commons.Color.menu.selectedText : Commons.Color.menu.text
          font.family: Style.font.family
          font.pixelSize: Style.font.body
        }
      }
    }

    Text {
      text: "j/k to choose, Space to toggle, Escape to close"
      color: Commons.Color.muted
      font.family: Style.font.family
      font.pixelSize: Style.font.body * 0.85
    }
  }
}
