#include "test-cpp-2.hpp"

Status test_cpp_2::test_2(
  const std::optional<std::uint32_t> &a,
  const std::optional<std::uint32_t> &b
)
{
  return Status::running();
}

int main(int argc, char *argv[])
{
}