#ifndef _ARORA_BUFFER_TYPES_HPP_
#define _ARORA_BUFFER_TYPES_HPP_

extern "C" {
  #include <arora/buffers.h>
}

#include <string>

namespace arora
{
  namespace buffer
  {
    // For array types.
    namespace detail {

      template<typename T, typename _ = void>
      struct is_container : std::false_type {};

      template<typename... Ts>
      struct is_container_helper {};

      template<typename T>
      struct is_container<
              T,
              std::conditional_t<
                  std::is_same<T, std::string>::value,
                  is_container_helper<
                      typename T::value_type,
                      typename T::size_type,
                      decltype(std::declval<T>().size()),
                      decltype(std::declval<T>().data())
                      >,
                  void
                  >
              > : public std::true_type {};

    } // ends namespace detail

    template<typename T>
    constexpr std::enable_if_t<!detail::is_container<T>::value, int> arora_buffer_type_of() noexcept;

    template<>
    constexpr std::enable_if_t<!detail::is_container<void>::value, int> arora_buffer_type_of<void>() noexcept { return ARORA_BUFFER_TYPE_UNIT; }

    template<>
    constexpr int arora_buffer_type_of<bool>() noexcept { return ARORA_BUFFER_TYPE_BOOLEAN; }

    template<>
    constexpr int arora_buffer_type_of<std::uint8_t>() noexcept { return ARORA_BUFFER_TYPE_U8; }

    template<>
    constexpr int arora_buffer_type_of<std::uint16_t>() noexcept { return ARORA_BUFFER_TYPE_U16; }

    template<>
    constexpr int arora_buffer_type_of<std::uint32_t>() noexcept { return ARORA_BUFFER_TYPE_U32; }

    template<>
    constexpr int arora_buffer_type_of<std::uint64_t>() noexcept { return ARORA_BUFFER_TYPE_U64; }

    template<>
    constexpr int arora_buffer_type_of<std::int8_t>() noexcept { return ARORA_BUFFER_TYPE_I8; }

    template<>
    constexpr int arora_buffer_type_of<std::int16_t>() noexcept { return ARORA_BUFFER_TYPE_I16; }

    template<>
    inline int arora_buffer_type_of<std::int32_t>() noexcept { return ARORA_BUFFER_TYPE_I32; }

    template<>
    constexpr int arora_buffer_type_of<std::int64_t>() noexcept { return ARORA_BUFFER_TYPE_I64; }

    template<>
    constexpr int arora_buffer_type_of<float>() noexcept { return ARORA_BUFFER_TYPE_F32; }

    template<>
    constexpr int arora_buffer_type_of<double>() noexcept { return ARORA_BUFFER_TYPE_F64; }

    template<>
    constexpr int arora_buffer_type_of<std::string>() noexcept { return ARORA_BUFFER_TYPE_STRING; }

    template<typename T>
    constexpr std::enable_if_t<detail::is_container<T>::value, int> arora_buffer_type_of() noexcept { return ARORA_BUFFER_TYPE_ARRAY; }
  }
}

#endif // _ARORA_BUFFER_TYPES_HPP_