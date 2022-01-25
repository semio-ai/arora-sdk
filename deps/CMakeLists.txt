include(ExternalProject)

set (MAKE_CONCURRENCY 10)

set (WASI_VERSION 14)
set (WASI_VERSION_FULL 14.0)
set (WASI_PLATFORM mingw)


ExternalProjecT_Add(wasi-sdk
  URL https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-${WASI_VERSION}/wasi-sdk-${WASI_VERSION_FULL}-${WASI_PLATFORM}.tar.gz
  CONFIGURE_COMMAND ""
  BUILD_COMMAND ""
  INSTALL_COMMAND ""
)