#include "test-cpp.hpp"

Status test_cpp::test_2_args(
  const std::experimental::optional<std::uint32_t> &a,
  const std::experimental::optional<std::uint32_t> &b
)
{
  return Status::success();
}

int main(int argc, char *argv[])
{
}