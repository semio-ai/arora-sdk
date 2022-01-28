#ifndef _ARORA_BUFFER_SERIALIZE_HPP_
#define _ARORA_BUFFER_SERIALIZE_HPP_

extern "C" {
  #include <arora/buffers.h>
}

#include <cstdint>
#include <optional>
#include <ranges>
#include <string_view>
#include <string>
#include "View.hpp"

namespace arora
{
  namespace buffer
  {
    // Templated helpers for writing to Arora buffers.
    template<typename T>
    int arora_buffer_type_of();

    template<typename T>
    void arora_buffer_writer_add_bulk(arora_buffer_writer *const writer, const T *const data, std::size_t size);
    
    template<typename T>
    void serialize(arora_buffer_writer *const writer, const T &value);

    template<std::ranges::contiguous_range R>
    void serialize(arora_buffer_writer *const writer, const R &range) {
      uintptr_t size = std::size(range); // also casting to Arora buffer size type.
      const auto *const data = range.cdata(); // if it was supported, I'd use std::ranges::cdata instead.
      using T = std::ranges::range_value_t<R>;
      arora_buffer_writer_add_array_primitive(writer, arora_buffer_type_of<T>(), size);
      arora_buffer_writer_add_bulk<T>(writer, data, size);
    }

    template<>
    void serialize<bool>(arora_buffer_writer *const writer, const bool &value)
    {
      arora_buffer_writer_add_boolean(writer, value);
    }

    template<>
    void serialize<std::uint8_t>(arora_buffer_writer *const writer, const std::uint8_t &value)
    {
      arora_buffer_writer_add_u8(writer, value);
    }

    template<>
    void serialize<std::uint16_t>(arora_buffer_writer *const writer, const std::uint16_t &value)
    {
      arora_buffer_writer_add_u16(writer, value);
    }

    template<>
    void serialize<std::uint32_t>(arora_buffer_writer *const writer, const std::uint32_t &value)
    {
      arora_buffer_writer_add_u32(writer, value);
    }

    template<>
    void serialize<std::uint64_t>(arora_buffer_writer *const writer, const std::uint64_t &value)
    {
      arora_buffer_writer_add_u64(writer, value);
    }

    template<>
    void serialize<std::int8_t>(arora_buffer_writer *const writer, const std::int8_t &value)
    {
      arora_buffer_writer_add_s8(writer, value);
    }

    template<>
    void serialize<std::int16_t>(arora_buffer_writer *const writer, const std::int16_t &value)
    {
      arora_buffer_writer_add_s16(writer, value);
    }

    template<>
    void serialize<std::int32_t>(arora_buffer_writer *const writer, const std::int32_t &value)
    {
      arora_buffer_writer_add_s32(writer, value);
    }

    template<>
    void serialize<std::int64_t>(arora_buffer_writer *const writer, const std::int64_t &value)
    {
      arora_buffer_writer_add_s64(writer, value);
    }

    template<>
    void serialize<float>(arora_buffer_writer *const writer, const float &value)
    {
      arora_buffer_writer_add_r32(writer, value);
    }

    template<>
    void serialize<double>(arora_buffer_writer *const writer, const double &value)
    {
      arora_buffer_writer_add_r64(writer, value);
    }

    template<>
    void serialize<std::string>(arora_buffer_writer *const writer, const std::string &value)
    {
      arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(value.data()), value.size());
    }

    template<>
    void serialize<std::string_view>(arora_buffer_writer *const writer, const std::string_view &value)
    {
      arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(value.data()), value.size());
    }
  }
}

#endif
