cmake_minimum_required(VERSION 3.14)

# Getting and using the WASI SDK
set(WASI_VERSION_FULL 25.0)
string(REGEX MATCH "^[0-9]+" WASI_VERSION "${WASI_VERSION_FULL}")

# Determine the host OS and architecture
# This is done approximately and manually, because CMake cannot determine it yet.
if (WIN32)
  set(WASI_HOST_OS "windows")
  # Use the Ninja generator by default
  set(CMAKE_GENERATOR "Ninja" CACHE STRING "Generator")
  set(CMAKE_MAKE_PROGRAM "ninja" CACHE STRING "Make program")
  set(WASI_HOST_ARCH "x86_64")
elseif (APPLE)
  set(WASI_HOST_OS "macos")
  execute_process(COMMAND uname -m OUTPUT_VARIABLE WASI_HOST_ARCH OUTPUT_STRIP_TRAILING_WHITESPACE)
elseif (UNIX)
  set(WASI_HOST_OS "linux")
  execute_process(COMMAND uname -m OUTPUT_VARIABLE WASI_HOST_ARCH OUTPUT_STRIP_TRAILING_WHITESPACE)
else ()
  message(FATAL "Unsupported platform")
endif()

include(FetchContent)
message(STATUS "Retrieving WASI SDK ${WASI_VERSION_FULL}...")
set(FETCHCONTENT_QUIET FALSE)
FetchContent_Declare(wasi_sdk
  URL https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-${WASI_VERSION}/wasi-sdk-${WASI_VERSION_FULL}-${WASI_HOST_ARCH}-${WASI_HOST_OS}.tar.gz
)
FetchContent_MakeAvailable(wasi_sdk)
set(WASI_SDK_PREFIX "${CMAKE_CURRENT_BINARY_DIR}/_deps/wasi_sdk-src" CACHE STRING "WASI SDK Prefix")
set(CMAKE_TOOLCHAIN_FILE "${WASI_SDK_PREFIX}/share/cmake/wasi-sdk.cmake" CACHE STRING "Toolchain file")
message(STATUS "WASI SDK is now available in ${WASI_SDK_PREFIX}")
