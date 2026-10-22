#include "example-cpp.hpp"

#include <vector>

namespace {

// Every sum returned so far, after a leading zero. The vector's constructor
// is a dynamic initializer: it runs once, from the module's `_initialize`,
// when a device loads the module, and the vector then lives across calls.
std::vector<std::int64_t> sums(1, 0);

} // namespace

// The sum of two 32-bit integers, as a 64-bit one: no sum overflows it.
std::int64_t example_cpp::add_two_integers(
  const std::optional<std::int32_t> &a,
  const std::optional<std::int32_t> &b
)
{
  const std::int64_t sum = std::int64_t{a.value_or(0)} + std::int64_t{b.value_or(0)};
  sums.push_back(sum);
  return sum;
}

std::uint32_t example_cpp::sums_so_far()
{
  return static_cast<std::uint32_t>(sums.size() - 1);
}
