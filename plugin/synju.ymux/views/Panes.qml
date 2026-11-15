import QtQuick
import Quickshell
import Quickshell.Hyprland
import Quickshell.Wayland
import qs.Commons as Commons
import "../Ymux.js" as Ymux

// Tile numbers (spec 5.5), like tmux `display-panes`: one surface per monitor with a
// big key over every visible tile. Only the focused monitor's surface takes the
// keyboard. A shown key focuses that tile; anything else, or the timeout, closes.
Item {
  id: root

  property bool active: false
  property var payload: ({ panes: [], timeout: 2000 })
  property string exe: ""
  signal done()

  readonly property string focusedName: Hyprland.focusedMonitor ? Hyprland.focusedMonitor.name : ""

  Timer {
    running: root.active
    interval: root.payload.timeout || 2000
    onTriggered: root.done()
  }

  Variants {
    model: Quickshell.screens

    PanelWindow {
      id: win
      required property var modelData
      screen: modelData
      visible: root.active
      color: "transparent"
      anchors { top: true; left: true; bottom: true; right: true }
      exclusionMode: ExclusionMode.Ignore
      WlrLayershell.layer: WlrLayer.Overlay
      WlrLayershell.namespace: "ymux-panes"
      WlrLayershell.keyboardFocus: (root.active && modelData.name === root.focusedName)
        ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

      Item {
        anchors.fill: parent
        focus: true
        Component.onCompleted: forceActiveFocus()
        Keys.onPressed: function(event) {
          var pane = Ymux.paneForKey(root.payload.panes || [], event.text)
          if (pane) Quickshell.execDetached([root.exe, "focus-window", pane.address])
          root.done()
          event.accepted = true
        }
      }

      Repeater {
        model: (root.payload.panes || []).filter(function(p) { return p.monitor === win.modelData.name })

        Rectangle {
          required property var modelData
          x: modelData.x + modelData.w / 2 - width / 2
          y: modelData.y + modelData.h / 2 - height / 2
          width: 96
          height: 96
          radius: 18
          color: Commons.Color.accent
          Text {
            anchors.centerIn: parent
            text: modelData.key
            color: Commons.Color.background
            font.pixelSize: 56
            font.bold: true
          }
        }
      }
    }
  }
}
