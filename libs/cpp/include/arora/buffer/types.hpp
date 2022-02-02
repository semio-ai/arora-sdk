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
    int arora_buffer_type_of();

    template<std::ranges::contiguous_range R>
    int arora_buffer_type_of() { return ARORA_BUFFER_TYPE_ARRAY; }

    template<>
    inline int arora_buffer_type_of<void>() { return ARORA_BUFFER_TYPE_UNIT; }

    template<>
    inline int arora_buffer_type_of<bool>() { return ARORA_BUFFER_TYPE_BOOLEAN; }

    template<>
    inline int arora_buffer_type_of<std::uint8_t>() { return ARORA_BUFFER_TYPE_U8; }

    template<>
    inline int arora_buffer_type_of<std::uint16_t>() { return ARORA_BUFFER_TYPE_U16; }

    template<>
    inline int arora_buffer_type_of<std::uint32_t>() { return ARORA_BUFFER_TYPE_U32; }

    template<>
    inline int arora_buffer_type_of<std::uint64_t>() { return ARORA_BUFFER_TYPE_U64; }

    template<>
    inline int arora_buffer_type_of<std::int8_t>() { return ARORA_BUFFER_TYPE_S8; }

    template<>
    inline int arora_buffer_type_of<std::int16_t>() { return ARORA_BUFFER_TYPE_S16; }

    template<>
    inline int arora_buffer_type_of<std::int32_t>() { return ARORA_BUFFER_TYPE_S32; }

    template<>
    inline int arora_buffer_type_of<std::int64_t>() { return ARORA_BUFFER_TYPE_S64; }

    template<>
    inline int arora_buffer_type_of<float>() { return ARORA_BUFFER_TYPE_R32; }

    template<>
    inline int arora_buffer_type_of<double>() { return ARORA_BUFFER_TYPE_R64; }

    template<>
    inline int arora_buffer_type_of<std::string>() { return ARORA_BUFFER_TYPE_STRING; }
  }
}

#endif // _ARORA_BUFFER_TYPES_HPP_