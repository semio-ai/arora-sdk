include(ExternalProject)

set (MAKE_CONCURRENCY 10)

set (WASI_VERSION 14)
set (WASI_VERSION_FULL 14.0)

if (${CMAKE_HOST_SYSTEM_NAME} MATCHES "Windows")
  set (WASI_PLATFORM mingw)
elseif (${CMAKE_HOST_SYSTEM_NAME} MATCHES "Darwin")
  set (WASI_PLATFORM macos)
elseif (${CMAKE_HOST_SYSTEM_NAME} MATCHES "Linux")
  set (WASI_PLATFORM linux)
endif()


ExternalProjecT_Add(wasi-sdk
  URL https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-${WASI_VERSION}/wasi-sdk-${WASI_VERSION_FULL}-${WASI_PLATFORM}.tar.gz
  CONFIGURE_COMMAND ""
  BUILD_COMMAND ""
  INSTALL_COMMAND ""
)