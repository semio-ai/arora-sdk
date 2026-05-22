// libqi stub — header-only, links but crashes on call.
//
// This is NOT a real implementation of libqi. Every callable function aborts
// at runtime with __builtin_trap(). The goal is to let consumers (currently
// just modules/nao) compile and link without dragging in the real libqi +
// Boost + OpenSSL build, which is too slow for iteration.
//
// When you actually need NAO functionality, swap this stub for the real libqi
// (see modules/nao/CMakeLists.txt — there is a build option to choose).

#pragma once

#include <string>
#include <utility>

namespace qi {

[[noreturn]] inline void _qi_stub_trap() { __builtin_trap(); }

inline void registerBaseTypes() { _qi_stub_trap(); }

class AnyObject {
public:
  template <typename Ret, typename... Args>
  Ret call(const std::string& /*name*/, Args&&... /*args*/) {
    _qi_stub_trap();
  }
};

template <typename T>
class Future {
public:
  T value() const { _qi_stub_trap(); }
};

class Session {
public:
  Session() = default;
  void connect(const std::string& /*url*/) { _qi_stub_trap(); }
  Future<AnyObject> service(const std::string& /*name*/) { _qi_stub_trap(); }
};

}  // namespace qi
