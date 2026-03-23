cmake_minimum_required(VERSION 3.16)
project(witty-terminal VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)

# Qt paths
set(CMAKE_PREFIX_PATH "/home/xuming/src/witty/6.7.3/gcc_arm64/lib/cmake")

# Find Qt packages
find_package(Qt6 REQUIRED COMPONENTS Core Gui Quick QuickControls2)

# Qt source files
set(SOURCES
    main.cpp
)

# QML resources - this creates the executable target
qt_add_executable(witty-terminal
    MANUAL_FINALIZATION
    main.cpp
)

qt_add_qml_module(witty-terminal
    URI Witty
    VERSION 1.0
    QML_FILES
        main.qml
    RESOURCES
        ../../assets/icon.png
)

# Link libraries
target_link_libraries(witty-terminal PRIVATE
    Qt6::Core
    Qt6::Gui
    Qt6::Quick
    Qt6::QuickControls2
)

qt_finalize_executable(witty-terminal)

# Install rules
install(TARGETS witty-terminal
    BUNDLE DESTINATION .
    LIBRARY DESTINATION ${CMAKE_INSTALL_LIBDIR}
)