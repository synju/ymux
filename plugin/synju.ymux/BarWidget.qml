import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Hyprland
import qs.Commons
import qs.Ui
import "Ymux.js" as Ymux

// tmux-style status line for this bar's monitor (spec 4), drawn as rounded pills
// like the user's tmux theme: a session pill with an icon cap, then one pill per
// workspace with its number in a coloured cap (accent when current), then markers.
// Colours come from the Omarchy theme.
BarWidget {
  id: root
  moduleName: "synju.ymux"

  readonly property var monitor: {
    var window = root.QsWindow.window
    var screen = window ? window.screen : null
    return screen ? Hyprland.monitorFor(screen) : Hyprland.focusedMonitor
  }
  readonly property string connector: root.monitor ? root.monitor.name : ""

  readonly property var service: {
    var sh = root.bar ? root.bar.shell : null
    return (sh && typeof sh.serviceFor === "function") ? sh.serviceFor("synju.ymux") : null
  }
  readonly property var row: (root.service && root.service.connected)
    ? Ymux.barRow(root.service.view, root.connector, root.vertical) : null

  readonly property string fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
  readonly property real fontSize: Style.font.body
  readonly property real pillHeight: Math.max(16, Math.round(root.barSize * 0.72))
  readonly property real pad: Style.spaceReal(2)
  // side padding inside every pill: half its height on each side (user request)
  readonly property real sidePad: Math.round(root.pillHeight / 2)
  readonly property color fg: root.bar ? root.bar.barForeground : Color.foreground
  readonly property color bodyColor: Qt.rgba(root.fg.r, root.fg.g, root.fg.b, 0.10)
  readonly property color capText: Color.background

  readonly property real trailingGap: root.vertical ? 0 : Style.spaceReal(1.5)
  implicitWidth: layout.implicitWidth + trailingGap
  implicitHeight: root.vertical ? layout.implicitHeight : root.barSize

  // one rounded pill: optional coloured cap on the left, optional body text
  component Pill: Item {
    id: pill
    property string capText: ""
    property string capFont: root.fontFamily
    property color capColor: Color.accent
    property string bodyText: ""
    // optional coloured dot drawn before the body text
    property bool dot: false
    property color dotColor: root.fg
    property color fill: root.bodyColor
    property bool clickable: false
    property real dim: 1
    signal clicked()

    implicitHeight: root.pillHeight
    implicitWidth: cap.width + body.width
    opacity: pill.dim

    Rectangle {
      anchors.fill: parent
      radius: height / 2
      color: pill.fill
      visible: pill.bodyText !== ""
    }

    Rectangle {
      id: cap
      visible: pill.capText !== ""
      width: visible ? capLabel.implicitWidth + root.sidePad * 2 : 0
      height: parent.height
      radius: height / 2
      color: pill.capColor
      Text {
        id: capLabel
        anchors.centerIn: parent
        text: pill.capText
        color: root.capText
        font.family: pill.capFont
        font.pixelSize: root.fontSize
        font.bold: true
      }
    }

    Item {
      id: body
      anchors.left: cap.right
      height: parent.height
      // the same side padding on every pill (user request)
      width: pill.bodyText !== "" ? bodyRow.implicitWidth + root.sidePad * 2 : 0
      Row {
        id: bodyRow
        anchors.centerIn: parent
        // a clear gap between the dot and the name (user request)
        spacing: root.pad * 3
        Text {
          visible: pill.dot
          text: "\u25cf"
          color: pill.dotColor
          font.family: root.fontFamily
          font.pixelSize: root.fontSize
        }
        Text {
          text: pill.bodyText
          color: root.fg
          font.family: root.fontFamily
          font.pixelSize: root.fontSize
        }
      }
    }

    MouseArea {
      anchors.fill: parent
      enabled: pill.clickable
      cursorShape: pill.clickable ? Qt.PointingHandCursor : Qt.ArrowCursor
      onClicked: pill.clicked()
    }
  }

  // a workspace pill like the user's tmux theme: name piece then number piece, each
  // with its own colour; active and inactive workspaces use different colours
  // (sampled from the user's tmux bar). An unnamed workspace has the number only.
  component WsPill: Item {
    id: ws
    property string name: ""
    property string number: ""
    property bool current: false
    property bool empty: false
    signal clicked()

    readonly property color nameFill: ws.current ? "#444446" : "#010101"
    readonly property color numberFill: ws.current ? "#C5A1EF" : "#8E94AC"
    readonly property color nameText: "#cdd6f4"
    readonly property color numberText: "#11111b"

    implicitHeight: root.pillHeight
    implicitWidth: namePiece.width + numberPiece.width
    opacity: ws.empty && !ws.current ? 0.75 : 1

    // the name piece: rounded on the left only
    Item {
      id: namePiece
      visible: ws.name !== ""
      width: visible ? nameLabel.implicitWidth + root.sidePad * 2 : 0
      height: parent.height
      Rectangle { anchors.fill: parent; radius: height / 2; color: ws.nameFill }
      Rectangle { anchors.right: parent.right; width: parent.width / 2; height: parent.height; color: ws.nameFill }
      Text {
        id: nameLabel
        anchors.centerIn: parent
        text: ws.name
        color: ws.nameText
        font.family: root.fontFamily
        font.pixelSize: root.fontSize
      }
    }

    // the number piece: rounded on the right, and on the left too when alone
    Item {
      id: numberPiece
      anchors.left: namePiece.right
      width: numberLabel.implicitWidth + root.sidePad * 2
      height: parent.height
      Rectangle { anchors.fill: parent; radius: height / 2; color: ws.numberFill }
      Rectangle {
        visible: namePiece.visible
        anchors.left: parent.left
        width: parent.width / 2
        height: parent.height
        color: ws.numberFill
      }
      Text {
        id: numberLabel
        anchors.centerIn: parent
        text: ws.number
        color: ws.numberText
        font.family: root.fontFamily
        font.pixelSize: root.fontSize
        font.bold: true
      }
    }

    MouseArea {
      anchors.fill: parent
      cursorShape: Qt.PointingHandCursor
      onClicked: ws.clicked()
    }
  }

  GridLayout {
    id: layout
    anchors.verticalCenter: root.vertical ? undefined : parent.verticalCenter
    anchors.left: parent.left
    columns: root.vertical ? 1 : 64
    // room between the pills (user request)
    columnSpacing: Style.space(5)
    rowSpacing: Style.space(2)

    // daemon down: one dim label, never a silent blank (spec 3.1)
    Pill {
      visible: root.row === null
      bodyText: "ymux"
      dim: 0.45
    }

    // session: icon cap plus name
    Pill {
      visible: root.row !== null && root.row.session !== ""
      // one piece: a dot then the name, on the tmux theme's surface colour (user choice)
      dot: true
      fill: "#313244"
      bodyText: root.row ? root.row.session : ""
    }

    Repeater {
      model: root.row ? root.row.items : []

      // workspace: name piece (only when the user named it) then number piece
      WsPill {
        required property var modelData
        name: root.vertical ? "" : modelData.name
        number: String(modelData.number)
        current: modelData.current
        empty: modelData.empty
        onClicked: if (root.service) root.service.run(["focus-id", String(modelData.id)])
      }
    }

    Repeater {
      model: root.row ? root.row.markers : []

      Pill {
        required property var modelData
        capText: modelData
        capColor: root.bar ? root.bar.urgent : Color.urgent
      }
    }
  }
}
