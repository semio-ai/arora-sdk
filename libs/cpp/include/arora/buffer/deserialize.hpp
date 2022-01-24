#ifndef _ARORA_BUFFER_DESERIALIZE_HPP_
#define _ARORA_BUFFER_DESERIALIZE_HPP_

extern "C" {
  #include <arora/buffers.h>
}

#include <cstdint>
#include <optional>
#include <string_view>

namespace arora
{
  namespace buffer
  {
    template<typename T>
    struct deserialize
    {
      std::optional<T> operator () (arora_buffer_reader *const reader) const;
    };

    template<>
    struct deserialize<bool>
    {
      std::optional<bool> operator () (arora_buffer_reader *const reader) const
      {
        return arora_buffer_reader_get_boolean(reader);
      }
    };

    template<>
    struct deserialize<std::uint8_t>
    {
      std::optional<std::uint8_t> operator () (arora_buffer_reader *const reader) const
      {
        return arora_buffer_reader_get_u8(reader);
      }
    };

    template<>
    struct deserialize<std::uint16_t>
    {
      std::optional<std::uint16_t> operator () (arora_buffer_reader *const reader) const
      {
        return arora_buffer_reader_get_u16(reader);
      }
    };

    template<>
    struct deserialize<std::uint32_t>
    {
      std::optional<std::uint32_t> operator () (arora_buffer_reader *const reader) const
      {
        return arora_buffer_reader_get_u32(reader);
      }
    };

    template<>
    struct deserialize<std::uint64_t>
    {
      std::optional<std::uint64_t> operator () (arora_buffer_reader *const reader) const
      {
        return arora_buffer_reader_get_u64(reader);
      }
    };

    template<>
    struct deserialize<std::int8_t>
    {
      std::optional<std::int8_t> operator () (arora_buffer_reader *const reader) const
      {
        return arora_buffer_reader_get_s8(reader);
      }
    };

    template<>
    struct deserialize<std::int16_t>
    {
      std::optional<std::int16_t> operator () (arora_buffer_reader *const reader) const
      {
        return arora_buffer_reader_get_s16(reader);
      }
    };

    template<>
    struct deserialize<std::int32_t>
    {
      std::optional<std::int32_t> operator () (arora_buffer_reader *const reader) const
      {
        return arora_buffer_reader_get_s32(reader);
      }
    };

    template<>
    struct deserialize<std::int64_t>
    {
      std::optional<std::int64_t> operator () (arora_buffer_reader *const reader) const
      {
        return arora_buffer_reader_get_s64(reader);
      }
    };

    template<>
    struct deserialize<float>
    {
      std::optional<float> operator () (arora_buffer_reader *const reader) const
      {
        return arora_buffer_reader_get_r32(reader);
      }
    };

    template<>
    struct deserialize<double>
    {
      std::optional<double> operator () (arora_buffer_reader *const reader) const
      {
        return arora_buffer_reader_get_r64(reader);
      }
    };

    template<>
    struct deserialize<std::string_view>
    {
      std::optional<std::string_view> operator () (arora_buffer_reader *const reader) const
      {
        std::uint32_t length = 0;
        const std::uint8_t *const data = arora_buffer_reader_get_string(reader, &length);
        if (data == nullptr) return std::nullopt;
        return std::string_view(reinterpret_cast<const char *>(data), length);
      }
    };
  }
}

#endif