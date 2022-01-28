#include "test-cpp.hpp"

std::int32_t test_cpp::test(
  const std::optional<Status> &a,
  const std::optional<std::int32_t> &b
)
{
  return a->is_success() ? 1 : 0;
}

int main(int argc, char *argv[])
{
}