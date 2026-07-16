cmake_minimum_required(VERSION 3.25)

project(htm_shell_compositor_host LANGUAGES C CXX)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_CXX_STANDARD 26)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

find_package(PkgConfig REQUIRED)
pkg_check_modules(HYPRLAND REQUIRED hyprland)
pkg_check_modules(WAYLAND_SERVER REQUIRED wayland-server)
find_program(WAYLAND_SCANNER wayland-scanner REQUIRED)

set(PROTOCOL_XML "${CMAKE_CURRENT_SOURCE_DIR}/../../protocols/htm-shell-v1.xml")
set(GENERATED_DIR "${CMAKE_CURRENT_BINARY_DIR}/generated")
set(PROTOCOL_HEADER "${GENERATED_DIR}/htm-shell-v1-server-protocol.h")
set(PROTOCOL_CODE "${GENERATED_DIR}/htm-shell-v1-protocol.c")

add_custom_command(
    OUTPUT "${PROTOCOL_HEADER}" "${PROTOCOL_CODE}"
    COMMAND "${CMAKE_COMMAND}" -E make_directory "${GENERATED_DIR}"
    COMMAND "${WAYLAND_SCANNER}" server-header "${PROTOCOL_XML}" "${PROTOCOL_HEADER}"
    COMMAND "${WAYLAND_SCANNER}" private-code "${PROTOCOL_XML}" "${PROTOCOL_CODE}"
    DEPENDS "${PROTOCOL_XML}"
    VERBATIM
)

add_library(htm-shell-compositor-host SHARED
    src/main.cpp
    "${PROTOCOL_CODE}"
)

target_include_directories(htm-shell-compositor-host PRIVATE "${GENERATED_DIR}")
target_include_directories(htm-shell-compositor-host SYSTEM PRIVATE
    ${HYPRLAND_INCLUDE_DIRS}
    ${WAYLAND_SERVER_INCLUDE_DIRS}
)
target_compile_options(htm-shell-compositor-host PRIVATE
    ${HYPRLAND_CFLAGS_OTHER}
    ${WAYLAND_SERVER_CFLAGS_OTHER}
    $<$<COMPILE_LANGUAGE:CXX>:-Wall;-Wextra;-Wpedantic;-Werror;-fno-gnu-unique>
)
target_link_directories(htm-shell-compositor-host PRIVATE
    ${WAYLAND_SERVER_LIBRARY_DIRS}
)
target_link_libraries(htm-shell-compositor-host PRIVATE
    ${WAYLAND_SERVER_LIBRARIES}
)
set_target_properties(htm-shell-compositor-host PROPERTIES
    PREFIX ""
    OUTPUT_NAME "htm-shell-compositor-host"
)
