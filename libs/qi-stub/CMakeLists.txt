cmake_minimum_required(VERSION 3.14)

# Header-only stub of libqi. Provides just enough surface to make
# modules/nao compile and link without fetching the real libqi (which pulls in
# Boost + OpenSSL and is the iteration killer). All functions trap at runtime.
add_library(qi-stub INTERFACE)
target_include_directories(qi-stub INTERFACE ${CMAKE_CURRENT_SOURCE_DIR}/include)

# Compatibility shim: the real libqi target is called `qi` and exposes
# QI_INCLUDE_DIRS. Mirror both so nao/CMakeLists.txt does not need to change.
add_library(qi ALIAS qi-stub)
set(QI_INCLUDE_DIRS ${CMAKE_CURRENT_SOURCE_DIR}/include CACHE INTERNAL "qi-stub include dirs")
