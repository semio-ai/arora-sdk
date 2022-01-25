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
    void skip(arora_buffer_reader *const reader, const std::uint8_t type);

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
        const std::uint8_t type = arora_buffer_reader_next_type(reader);
        if (type == ARORA_BUFFER_TYPE_BOOLEAN)
        {
          return arora_buffer_reader_get_boolean(reader);
        }
        else
        {
          skip(reader, type);
          return std::nullopt;
        }
      }
    };

    template<>
    struct deserialize<std::uint8_t>
    {
      std::optional<std::uint8_t> operator () (arora_buffer_reader *const reader) const
      {
        const std::uint8_t type = arora_buffer_reader_next_type(reader);
        if (type == ARORA_BUFFER_TYPE_U8)
        {
          return arora_buffer_reader_get_u8(reader);
        }
        else
        {
          skip(reader, type);
          return std::nullopt;
        }
      }
    };

    template<>
    struct deserialize<std::uint16_t>
    {
      std::optional<std::uint16_t> operator () (arora_buffer_reader *const reader) const
      {
        const std::uint8_t type = arora_buffer_reader_next_type(reader);
        if (type == ARORA_BUFFER_TYPE_U16)
        {
          return arora_buffer_reader_get_u16(reader);
        }
        else
        {
          skip(reader, type);
          return std::nullopt;
        }
      }
    };

    template<>
    struct deserialize<std::uint32_t>
    {
      std::optional<std::uint32_t> operator () (arora_buffer_reader *const reader) const
      {
        const std::uint8_t type = arora_buffer_reader_next_type(reader);
        if (type == ARORA_BUFFER_TYPE_U32)
        {
          return arora_buffer_reader_get_u32(reader);
        }
        else
        {
          skip(reader, type);
          return std::nullopt;
        }
      }
    };

    template<>
    struct deserialize<std::uint64_t>
    {
      std::optional<std::uint64_t> operator () (arora_buffer_reader *const reader) const
      {
        const std::uint8_t type = arora_buffer_reader_next_type(reader);
        if (type == ARORA_BUFFER_TYPE_U64)
        {
          return arora_buffer_reader_get_u64(reader);
        }
        else
        {
          skip(reader, type);
          return std::nullopt;
        }
      }
    };

    template<>
    struct deserialize<std::int8_t>
    {
      std::optional<std::int8_t> operator () (arora_buffer_reader *const reader) const
      {
        const std::uint8_t type = arora_buffer_reader_next_type(reader);
        if (type == ARORA_BUFFER_TYPE_S8)
        {
          return arora_buffer_reader_get_s8(reader);
        }
        else
        {
          skip(reader, type);
          return std::nullopt;
        }
      }
    };

    template<>
    struct deserialize<std::int16_t>
    {
      std::optional<std::int16_t> operator () (arora_buffer_reader *const reader) const
      {
        const std::uint8_t type = arora_buffer_reader_next_type(reader);
        if (type == ARORA_BUFFER_TYPE_S16)
        {
          return arora_buffer_reader_get_s16(reader);
        }
        else
        {
          skip(reader, type);
          return std::nullopt;
        }
      }
    };

    template<>
    struct deserialize<std::int32_t>
    {
      std::optional<std::int32_t> operator () (arora_buffer_reader *const reader) const
      {
        const std::uint8_t type = arora_buffer_reader_next_type(reader);
        if (type == ARORA_BUFFER_TYPE_S32)
        {
          return arora_buffer_reader_get_s32(reader);
        }
        else
        {
          skip(reader, type);
          return std::nullopt;
        }
      }
    };

    template<>
    struct deserialize<std::int64_t>
    {
      std::optional<std::int64_t> operator () (arora_buffer_reader *const reader) const
      {
        const std::uint8_t type = arora_buffer_reader_next_type(reader);
        if (type == ARORA_BUFFER_TYPE_S64)
        {
          return arora_buffer_reader_get_s64(reader);
        }
        else
        {
          skip(reader, type);
          return std::nullopt;
        }
      }
    };

    template<>
    struct deserialize<float>
    {
      std::optional<float> operator () (arora_buffer_reader *const reader) const
      {
        const std::uint8_t type = arora_buffer_reader_next_type(reader);
        if (type == ARORA_BUFFER_TYPE_R32)
        {
          return arora_buffer_reader_get_r32(reader);
        }
        else
        {
          skip(reader, type);
          return std::nullopt;
        }
      }
    };

    template<>
    struct deserialize<double>
    {
      std::optional<double> operator () (arora_buffer_reader *const reader) const
      {
        const std::uint8_t type = arora_buffer_reader_next_type(reader);
        if (type == ARORA_BUFFER_TYPE_R64)
        {
          return arora_buffer_reader_get_r64(reader);
        }
        else
        {
          skip(reader, type);
          return std::nullopt;
        }
      }
    };

    template<>
    struct deserialize<std::string_view>
    {
      std::optional<std::string_view> operator () (arora_buffer_reader *const reader) const
      {
        const std::uint8_t type = arora_buffer_reader_next_type(reader);
        if (type == ARORA_BUFFER_TYPE_STRING)
        {
          std::uint32_t length = 0;
          const std::uint8_t *const data = arora_buffer_reader_get_string(reader, &length);
          if (data == nullptr) return std::nullopt;
          return std::string_view(reinterpret_cast<const char *>(data), length);
        }
        else
        {
          skip(reader, type);
          return std::nullopt;
        }
      }
    };

  }
}

#endif