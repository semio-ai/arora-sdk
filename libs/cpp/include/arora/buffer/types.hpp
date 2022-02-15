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
    // Templated helpers for writing to Arora buffers.
    template<typename T>
    int arora_buffer_type_of() noexcept;

    template<std::ranges::contiguous_range R>
    int arora_buffer_type_of() noexcept { return ARORA_BUFFER_TYPE_ARRAY; }

    template<>
    inline int arora_buffer_type_of<void>() noexcept { return ARORA_BUFFER_TYPE_UNIT; }

    template<>
    inline int arora_buffer_type_of<bool>() noexcept { return ARORA_BUFFER_TYPE_BOOLEAN; }

    template<>
    inline int arora_buffer_type_of<std::uint8_t>() noexcept { return ARORA_BUFFER_TYPE_U8; }

    template<>
    inline int arora_buffer_type_of<std::uint16_t>() noexcept { return ARORA_BUFFER_TYPE_U16; }

    template<>
    inline int arora_buffer_type_of<std::uint32_t>() noexcept { return ARORA_BUFFER_TYPE_U32; }

    template<>
    inline int arora_buffer_type_of<std::uint64_t>() noexcept { return ARORA_BUFFER_TYPE_U64; }

    template<>
    inline int arora_buffer_type_of<std::int8_t>() noexcept { return ARORA_BUFFER_TYPE_I8; }

    template<>
    inline int arora_buffer_type_of<std::int16_t>() noexcept { return ARORA_BUFFER_TYPE_I16; }

    template<>
    inline int arora_buffer_type_of<std::int32_t>() noexcept { return ARORA_BUFFER_TYPE_I32; }

    template<>
    inline int arora_buffer_type_of<std::int64_t>() noexcept { return ARORA_BUFFER_TYPE_I64; }

    template<>
    inline int arora_buffer_type_of<float>() noexcept { return ARORA_BUFFER_TYPE_F32; }

    template<>
    inline int arora_buffer_type_of<double>() noexcept { return ARORA_BUFFER_TYPE_F64; }

    template<>
    inline int arora_buffer_type_of<std::string>() noexcept { return ARORA_BUFFER_TYPE_STRING; }
  }
}

#endif // _ARORA_BUFFER_TYPES_HPP_