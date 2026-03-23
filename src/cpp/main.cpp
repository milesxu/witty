#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QIcon>
#include <QQuickWindow>
#include <QCommandLineParser>
#include <QDebug>

int main(int argc, char *argv[])
{
    // High-DPI scaling is always enabled in Qt 6

    QGuiApplication app(argc, argv);
    app.setApplicationName("Witty Terminal");
    app.setOrganizationName("Witty");
    app.setApplicationVersion("0.1.0");

    // Set application icon
    QIcon icon;
    icon.addFile(":/assets/icon.png");
    app.setWindowIcon(icon);

    // Parse command line arguments
    QCommandLineParser parser;
    parser.setApplicationDescription("Witty Terminal - AI-powered terminal emulator");
    parser.addHelpOption();
    parser.addVersionOption();
    parser.process(app);

    // Load QML
    QQmlApplicationEngine engine;

    // Import path for QML modules
    const QUrl url(QStringLiteral("qrc:/Witty/main.qml"));

    QObject::connect(&engine, &QQmlApplicationEngine::objectCreated,
                     &app, [url](QObject *obj, const QUrl &objUrl) {
        if (!obj && url == objUrl)
            QCoreApplication::exit(-1);
    }, Qt::QueuedConnection);

    engine.load(url);

    if (engine.rootObjects().isEmpty()) {
        qCritical() << "Failed to load QML file";
        return -1;
    }

    qInfo() << "Witty Terminal started successfully";

    return app.exec();
}