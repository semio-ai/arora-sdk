cmake_minimum_required(VERSION 3.20)
project(nao)
add_executable(nao src/nao.cpp)
target_link_options(nao PRIVATE -static-libgcc -static-libstdc++ -static)
install(TARGETS nao DESTINATION .)

# Makes Qi available.
# Also makes OpenSSL and Boost available.
include(FetchContent)
FetchContent_Declare(
  qi
  GIT_REPOSITORY https://github.com/semio-ai/libqi.git
  GIT_TAG d9e939fd88b2fb26da8c3ba6f6bec356e143fba9
)
FetchContent_MakeAvailable(qi)

add_dependencies(nao qi)
target_include_directories(
  nao
  PRIVATE
  ${QI_INCLUDE_DIRS}
)

find_package(Threads REQUIRED)
target_link_libraries(nao PUBLIC qi
  Threads::Threads
  ${CMAKE_THREAD_LIBS_INIT}
  ${CMAKE_DL_LIBS}
)
