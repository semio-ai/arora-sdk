#ifndef _ARORA_BUFFER_SERIALIZE_HPP_
#define _ARORA_BUFFER_SERIALIZE_HPP_

extern "C" {
  #include <arora/buffers.h>
}

#include <cstdint>
#include <optional>
#include <string_view>
#include <string>

namespace arora
{
  namespace buffer
  {
    template<typename T>
    struct serialize
    {
      void operator () (arora_buffer_writer *const writer, const T &value) const;
    };

    template<>
    struct serialize<bool>
    {
      void operator () (arora_buffer_writer *const writer, const bool &value) const
      {
        arora_buffer_writer_add_boolean(writer, value);
      }
    };

    template<>
    struct serialize<std::uint8_t>
    {
      void operator () (arora_buffer_writer *const writer, const std::uint8_t value) const
      {
        return arora_buffer_writer_add_u8(writer, value);
      }
    };

    template<>
    struct serialize<std::uint16_t>
    {
      void operator () (arora_buffer_writer *const writer, const std::uint16_t value) const
      {
        return arora_buffer_writer_add_u16(writer, value);
      }
    };

    template<>
    struct serialize<std::uint32_t>
    {
      void operator () (arora_buffer_writer *const writer, const std::uint32_t value) const
      {
        return arora_buffer_writer_add_u32(writer, value);
      }
    };

    template<>
    struct serialize<std::uint64_t>
    {
      void operator () (arora_buffer_writer *const writer, const std::uint64_t value) const
      {
        return arora_buffer_writer_add_u64(writer, value);
      }
    };

    template<>
    struct serialize<std::int8_t>
    {
      void operator () (arora_buffer_writer *const writer, const std::int8_t value) const
      {
        return arora_buffer_writer_add_s8(writer, value);
      }
    };

    template<>
    struct serialize<std::int16_t>
    {
      void operator () (arora_buffer_writer *const writer, const std::int16_t value) const
      {
        return arora_buffer_writer_add_s16(writer, value);
      }
    };

    template<>
    struct serialize<std::int32_t>
    {
      void operator () (arora_buffer_writer *const writer, const std::int32_t value) const
      {
        return arora_buffer_writer_add_s32(writer, value);
      }
    };

    template<>
    struct serialize<std::int64_t>
    {
      void operator () (arora_buffer_writer *const writer, const std::int64_t value) const
      {
        return arora_buffer_writer_add_s64(writer, value);
      }
    };

    template<>
    struct serialize<float>
    {
      void operator () (arora_buffer_writer *const writer, const float value) const
      {
        return arora_buffer_writer_add_r32(writer, value);
      }
    };

    template<>
    struct serialize<double>
    {
      void operator () (arora_buffer_writer *const writer, const double value) const
      {
        return arora_buffer_writer_add_r64(writer, value);
      }
    };

    template<>
    struct serialize<std::string>
    {
      void operator () (arora_buffer_writer *const writer, const std::string &value) const
      {
        return arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(value.data()), value.size());
      }
    };
  }
}

#endif