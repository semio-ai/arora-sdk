cmake_minimum_required(VERSION 3.14)

# Getting and using the WASI SDK
set(WASI_VERSION 14)
set(WASI_VERSION_FULL "${WASI_VERSION}.0")
if (WIN32)
  set(WASI_PLATFORM "mingw")
  # Use the Ninja generator by default
  set(CMAKE_GENERATOR "Ninja" CACHE STRING "Generator")
  set(CMAKE_MAKE_PROGRAM "ninja" CACHE STRING "Make program")
elseif (APPLE)
  set(WASI_PLATFORM "macos")
elseif (UNIX)
  set(WASI_PLATFORM "linux")
else ()
  message(FATAL "Unsupported platform")
endif()

include(FetchContent)
message(STATUS "Retrieving WASI SDK ${WASI_VERSION_FULL}...")
set(FETCHCONTENT_QUIET FALSE)
FetchContent_Declare(wasi_sdk
  URL https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-${WASI_VERSION}/wasi-sdk-${WASI_VERSION_FULL}-${WASI_PLATFORM}.tar.gz
)
FetchContent_MakeAvailable(wasi_sdk)
set(WASI_SDK_PREFIX "${CMAKE_CURRENT_BINARY_DIR}/_deps/wasi_sdk-src" CACHE STRING "WASI SDK Prefix")
set(CMAKE_TOOLCHAIN_FILE "${WASI_SDK_PREFIX}/share/cmake/wasi-sdk.cmake" CACHE STRING "Toolchain file")
message(STATUS "WASI SDK is now available in ${WASI_SDK_PREFIX}")
