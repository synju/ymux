import QtQuick
import Quickshell.Io
import qs.Commons as Commons
import qs.Commons
import "../Ymux.js" as Ymux

// One-line prompt (spec 5.2): prefilled with the current name; Enter runs the
// matching ymux command, Escape cancels. An error from ymux stays visible
// under the field instead of closing.
Rectangle {
  id: card

  property bool active: false
  property var payload: ({ view: "" })
  property string exe: ""
  property string error: ""
  signal done()

  width: 420
  height: column.implicitHeight + 32
  radius: 12
  color: Commons.Color.menu.background
  border.color: Commons.Color.menu.border
  border.width: 1

  onActiveChanged: {
    if (card.active) {
      card.error = ""
      input.text = card.payload.current || ""
      input.selectAll()
      input.forceActiveFocus()
    }
  }

  // swallow clicks so they do not reach the scrim
  MouseArea { anchors.fill: parent }

  Column {
    id: column
    anchors.fill: parent
    anchors.margins: 16
    spacing: 10

    Text {
      text: Ymux.promptTitle(card.payload)
      color: Commons.Color.menu.text
      font.family: Style.font.family
      font.pixelSize: Style.font.body
      font.bold: true
    }

    Rectangle {
      width: parent.width
      height: input.implicitHeight + 14
      radius: 8
      color: Commons.Color.menu.selectedBackground
      border.color: Commons.Color.menu.border
      border.width: 1

      TextInput {
        id: input
        anchors.fill: parent
        anchors.margins: 7
        color: Commons.Color.menu.text
        selectionColor: Commons.Color.accent
        selectedTextColor: Commons.Color.background
        font.family: Style.font.family
        font.pixelSize: Style.font.body
        clip: true
        // Leader + `.`: whole numbers of 1 or more only (user request)
        validator: Ymux.isNumeric(card.payload) ? numberOnly : null
        inputMethodHints: Ymux.isNumeric(card.payload) ? Qt.ImhDigitsOnly : Qt.ImhNone
        Keys.onPressed: function(event) {
          if (event.key === Qt.Key_Escape) {
            card.done()
            event.accepted = true
          } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
            if (!submit.running && input.acceptableInput) {
              submit.command = [card.exe, "--no-notify"].concat(Ymux.promptCommand(card.payload, input.text.trim()))
              submit.running = true
            }
            event.accepted = true
          }
        }
      }
    }

    Text {
      visible: card.error !== ""
      width: parent.width
      wrapMode: Text.Wrap
      text: card.error
      color: Commons.Color.urgent
      font.family: Style.font.family
      font.pixelSize: Style.font.body
    }

    Text {
      text: "Enter to save, Escape to cancel"
      color: Commons.Color.muted
      font.family: Style.font.family
      font.pixelSize: Style.font.body * 0.85
    }
  }

  RegularExpressionValidator {
    id: numberOnly
    regularExpression: /[1-9][0-9]{0,5}/
  }

  Process {
    id: submit
    stdout: StdioCollector { id: out }
    onExited: function(code) {
      if (code === 0) card.done()
      else card.error = out.text.trim() || "ymux: could not save"
    }
  }
}
