#include "example-cpp.hpp"

#include <vector>

namespace {

// Every sum returned so far, after a leading zero. The vector's constructor
// is a dynamic initializer: it runs once, from the module's `_initialize`,
// when a device loads the module, and the vector then lives across calls.
std::vector<std::int32_t> sums(1, 0);

} // namespace

std::int32_t example_cpp::add_two_integers(
  const std::optional<std::int32_t> &a,
  const std::optional<std::int32_t> &b
)
{
  const std::int32_t sum = a.value_or(0) + b.value_or(0);
  sums.push_back(sum);
  return sum;
}

std::uint32_t example_cpp::sums_so_far()
{
  return static_cast<std::uint32_t>(sums.size() - 1);
}
