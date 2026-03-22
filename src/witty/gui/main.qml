import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ApplicationWindow {
    visible: true
    width: 640
    height: 480
    title: qsTr("Witty Terminal - Hello World")

    // Shortcut: Ctrl+Q to quit
    Shortcut {
        sequence: StandardKey.Quit
        context: Qt.ApplicationShortcut
        onActivated: Qt.quit()
    }

    ColumnLayout {
        anchors.centerIn: parent
        spacing: 20

        Label {
            text: qsTr("Hello, World! 👋")
            font.pixelSize: 32
            font.bold: true
            Layout.alignment: Qt.AlignHCenter
        }

        Label {
            text: qsTr("Welcome to Witty Terminal")
            font.pixelSize: 18
            color: "#666"
            Layout.alignment: Qt.AlignHCenter
        }

        Button {
            text: qsTr("Click Me")
            Layout.alignment: Qt.AlignHCenter
            onClicked: {
                label.text = qsTr("Button clicked! 🎉")
                label.color = "#4CAF50"
            }
        }

        Label {
            id: label
            text: qsTr("")
            font.pixelSize: 16
            Layout.alignment: Qt.AlignHCenter
        }
    }

    background: Rectangle {
        color: "#f5f5f5"
    }
}