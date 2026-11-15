import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons as Commons
import qs.Ui
import "Ymux.js" as Ymux
import "views"

// Menu root of the ymux plugin (spec 5). The shell keeps it loaded and calls
// open(payloadJson) on every `omarchy-shell shell summon synju.ymux '<json>'`;
// the payload's `view` picks what to show.
Item {
  id: root

  property var shell: null
  property var manifest: null
  property var service: null
  property bool opened: false
  property var payload: ({ view: "" })
  readonly property string exe: Quickshell.env("HOME") + "/.local/bin/ymux"
  readonly property string view: root.opened ? root.payload.view : ""

  function open(payloadJson) {
    root.payload = Ymux.parsePayload(payloadJson)
    var v = root.payload.view
    root.opened = Ymux.opensPrompt(root.payload) || v === "tree" || v === "options" || v === "panes" || v === "help"
  }

  function close() {
    root.opened = false
  }

  function finish() {
    root.close()
    if (root.shell && typeof root.shell.hide === "function") root.shell.hide("synju.ymux")
  }

  OverlayWindow {
    id: panel
    shown: root.opened && root.view !== "panes"
    WlrLayershell.namespace: "ymux-overlay"

    Rectangle {
      anchors.fill: parent
      color: Commons.Color.menu.scrim
    }

    MouseArea {
      anchors.fill: parent
      onClicked: root.finish()
    }

    Prompt {
      anchors.centerIn: parent
      visible: Ymux.opensPrompt(root.payload)
      active: panel.shown && visible
      payload: root.payload
      exe: root.exe
      onDone: root.finish()
    }

    Tree {
      anchors.centerIn: parent
      visible: root.view === "tree"
      active: panel.shown && visible
      tree: (root.service && root.service.view) ? root.service.view.tree : []
      folded: (root.service && root.service.view) ? root.service.view.folded : []
      onRenameRow: function(payload) { root.payload = payload }
      exe: root.exe
      onDone: root.finish()
    }

    Help {
      anchors.centerIn: parent
      visible: root.view === "help"
      active: panel.shown && visible
      payload: root.payload
      onDone: root.finish()
    }

    Options {
      anchors.centerIn: parent
      visible: root.view === "options"
      active: panel.shown && visible
      relaunch: (root.service && root.service.view) ? root.service.view.relaunch : true
      exe: root.exe
      onDone: root.finish()
    }
  }

  Panes {
    active: root.view === "panes"
    payload: root.payload
    exe: root.exe
    onDone: root.finish()
  }
}
