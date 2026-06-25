#include "test-cpp-2.hpp"
#include "test-cpp.hpp"
#include <iostream>

std::int32_t test_cpp_2::test_structured(
  const std::optional<Status> &a,
  const std::optional<TestStructure1> &b
)
{
  printf("Is success? %d\n", a->is_success());
  printf("Is test2 success? %d\n", test_cpp::test_2_args(1, 2)->is_success());
  return a->is_success() ? 1 : 0;
}

int main(int argc, char *argv[])
{
}
