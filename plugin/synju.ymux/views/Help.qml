import QtQuick
import Quickshell
import qs.Commons as Commons
import qs.Commons
import "../Ymux.js" as Ymux

// Leader + ? (user request 2026-11-16): every ymux key, grouped, from the live config.
// j/k or arrows scroll; any other key or a click closes.
Rectangle {
  id: card

  property bool active: false
  property var payload: ({ groups: [] })
  readonly property var rows: Ymux.helpRows(card.payload)
  readonly property real rowHeight: Math.round(Style.font.body * 1.8)
  signal done()

  width: 720
  height: Math.min(640, card.rows.length * card.rowHeight + title.height + 56)
  radius: 12
  color: Commons.Color.menu.background
  border.color: Commons.Color.menu.border
  border.width: 1

  onActiveChanged: if (card.active) {
    list.positionViewAtBeginning()
    keys.forceActiveFocus()
  }

  MouseArea { anchors.fill: parent; onClicked: card.done() }

  Item {
    id: keys
    anchors.fill: parent
    focus: true
    Keys.onPressed: function(event) {
      if (event.key === Qt.Key_Down || event.text === "j") list.flick(0, -800)
      else if (event.key === Qt.Key_Up || event.text === "k") list.flick(0, 800)
      else card.done()
      event.accepted = true
    }
  }

  Text {
    id: title
    x: 16
    y: 14
    text: "ymux keys"
    color: Commons.Color.menu.text
    font.family: Style.font.family
    font.pixelSize: Style.font.body
    font.bold: true
  }

  // scrollbar: shown whenever the list is longer than the card (user request)
  Rectangle {
    id: bar
    readonly property var thumb: Ymux.scrollThumb(list.visibleArea.yPosition, list.visibleArea.heightRatio, bar.height)
    visible: bar.thumb.visible
    anchors.top: list.top
    anchors.bottom: list.bottom
    anchors.right: parent.right
    anchors.rightMargin: 10
    width: 6
    radius: 3
    color: Qt.rgba(Commons.Color.menu.text.r, Commons.Color.menu.text.g, Commons.Color.menu.text.b, 0.12)
    Rectangle {
      width: parent.width
      radius: 3
      y: bar.thumb.y
      height: bar.thumb.height
      color: Commons.Color.accent
    }
  }

  ListView {
    id: list
    anchors.top: title.bottom
    anchors.topMargin: 10
    anchors.left: parent.left
    anchors.right: bar.left
    anchors.rightMargin: 8
    anchors.bottom: parent.bottom
    anchors.margins: 16
    clip: true
    model: card.rows
    boundsBehavior: Flickable.StopAtBounds

    delegate: Item {
      required property var modelData
      width: ListView.view.width
      height: card.rowHeight

      Text {
        visible: !!modelData.header
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 2
        text: modelData.header || ""
        color: Commons.Color.accent
        font.family: Style.font.family
        font.pixelSize: Style.font.body * 0.9
        font.bold: true
      }

      Text {
        id: keyText
        visible: !modelData.header
        anchors.verticalCenter: parent.verticalCenter
        x: 12
        width: 260
        elide: Text.ElideRight
        text: modelData.keys || ""
        color: Commons.Color.menu.text
        font.family: Style.font.family
        font.pixelSize: Style.font.body
        font.bold: true
      }

      Text {
        visible: !modelData.header
        anchors.verticalCenter: parent.verticalCenter
        anchors.left: keyText.right
        anchors.leftMargin: 16
        anchors.right: parent.right
        elide: Text.ElideRight
        text: modelData.text || ""
        color: Commons.Color.menu.text
        font.family: Style.font.family
        font.pixelSize: Style.font.body
      }
    }
  }
}
