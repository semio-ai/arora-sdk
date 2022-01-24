project(arora-cpp)
cmake_minimum_required(VERSION 3.0)

file (GLOB_RECURSE SOURCES "src/*.cpp")

install(DIRECTORIES include DESTINATION include)