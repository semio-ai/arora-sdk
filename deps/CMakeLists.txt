include(ExternalProject)

ExternalProject_Add(binaryen
  SOURCE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/binaryen"
  BINARY_DIR "${CMAKE_CURRENT_SOURCE_DIR}/binaryen"
  INSTALL_COMMAND ""
)