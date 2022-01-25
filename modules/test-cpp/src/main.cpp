#include "test-cpp.hpp"

std::int32_t test_cpp::test(
  const std::optional<std::int32_t> &a,
  const std::optional<std::int32_t> &b
)
{
  if (!a || !b) return 0;
  
  return *a + *b;
}

int main(int argc, char *argv[])
{
}