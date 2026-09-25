#include "test-cpp.hpp"

bool test_cpp::test_2_args(
  const std::optional<std::uint32_t> &a,
  const std::optional<std::uint32_t> &b
)
{
  return true;
}

std::optional<std::uint32_t> test_cpp::test_optional(
  const std::optional<std::uint32_t> &a
)
{
  if (a) {
    return *a + 1;
  }
  return std::nullopt;
}

int main(int argc, char *argv[])
{
}
