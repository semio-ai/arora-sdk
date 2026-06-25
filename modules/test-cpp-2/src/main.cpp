#include "test-cpp-2.hpp"
#include "test-cpp.hpp"
#include <iostream>

std::int32_t test_cpp_2::test_structured(
  const std::optional<bool> &a,
  const std::optional<TestStructure1> &b
)
{
  printf("a = %d\n", a.value_or(false));
  printf("test2 = %d\n", test_cpp::test_2_args(1, 2).value_or(false));
  return a.value_or(false) ? 1 : 0;
}

int main(int argc, char *argv[])
{
}
