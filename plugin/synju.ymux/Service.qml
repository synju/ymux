import QtQuick
import Quickshell
import Quickshell.Io
import "Ymux.js" as Ymux

// One instance for the whole shell: holds the single `ymux watch` stream
// (spec 3 item 3) and runs CLI commands for the widgets.
Item {
  id: root

  property var shell: null
  property var manifest: null
  property bool connected: false
  property var view: null
  readonly property string exe: Quickshell.env("HOME") + "/.local/bin/ymux"

  function run(args) {
    Quickshell.execDetached([root.exe].concat(args))
  }

  Process {
    id: watch
    command: [root.exe, "watch"]
    running: true
    stdout: SplitParser {
      onRead: function(line) {
        var v = Ymux.parseView(line)
        if (v) {
          root.view = v
          root.connected = true
        }
      }
    }
    onExited: {
      root.connected = false
      retry.start()
    }
  }

  // The daemon is down or restarting: try again every 2 s (spec 3.1).
  Timer {
    id: retry
    interval: 2000
    repeat: false
    onTriggered: watch.running = true
  }
}
