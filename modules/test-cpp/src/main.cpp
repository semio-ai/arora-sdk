#include "test-cpp.hpp"

std::optional<std::int32_t> test_cpp::test(
  const std::optional<std::int32_t> &a,
  const std::optional<std::int32_t> &b
)
{
  if (!a || !b) return std::nullopt;
  return *a + *b;
}