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
#include "types.hpp"
#include "View.hpp"

namespace arora
{
  namespace buffer
  {
    template<typename T>
    void serialize(arora_buffer_writer *const writer, const T &value) noexcept;

    template<typename T>
    void arora_buffer_writer_add_bulk(arora_buffer_writer *const writer, const T *const data, std::size_t size) noexcept {
      auto end = data + size;
      for (auto* it = data; it < end; ++it) {
        serialize(writer, *data);
      }
    }

    template<std::ranges::contiguous_range R>
    void serialize(arora_buffer_writer *const writer, const R &range) noexcept {
      uintptr_t size = std::size(range); // also casting to Arora buffer size type.
      const auto *const data = range.data(); // if it was supported, I'd use std::ranges::cdata instead.
      using T = std::ranges::range_value_t<R>;
      arora_buffer_writer_add_array_primitive(writer, arora_buffer_type_of<T>(), size);
      arora_buffer_writer_add_bulk<T>(writer, data, size);
    }

    template<>
    inline void serialize<bool>(arora_buffer_writer *const writer, const bool &value) noexcept
    {
      arora_buffer_writer_add_boolean(writer, value);
    }

    template<>
    inline void serialize<std::uint8_t>(arora_buffer_writer *const writer, const std::uint8_t &value) noexcept
    {
      arora_buffer_writer_add_u8(writer, value);
    }

    template<>
    inline void serialize<std::uint16_t>(arora_buffer_writer *const writer, const std::uint16_t &value) noexcept
    {
      arora_buffer_writer_add_u16(writer, value);
    }

    template<>
    inline void serialize<std::uint32_t>(arora_buffer_writer *const writer, const std::uint32_t &value) noexcept
    {
      arora_buffer_writer_add_u32(writer, value);
    }

    template<>
    inline void serialize<std::uint64_t>(arora_buffer_writer *const writer, const std::uint64_t &value) noexcept
    {
      arora_buffer_writer_add_u64(writer, value);
    }

    template<>
    inline void serialize<std::int8_t>(arora_buffer_writer *const writer, const std::int8_t &value) noexcept
    {
      arora_buffer_writer_add_i8(writer, value);
    }

    template<>
    inline void serialize<std::int16_t>(arora_buffer_writer *const writer, const std::int16_t &value) noexcept
    {
      arora_buffer_writer_add_i16(writer, value);
    }

    template<>
    inline void serialize<std::int32_t>(arora_buffer_writer *const writer, const std::int32_t &value) noexcept
    {
      arora_buffer_writer_add_i32(writer, value);
    }

    template<>
    inline void serialize<std::int64_t>(arora_buffer_writer *const writer, const std::int64_t &value) noexcept
    {
      arora_buffer_writer_add_i64(writer, value);
    }

    template<>
    inline void serialize<float>(arora_buffer_writer *const writer, const float &value) noexcept
    {
      arora_buffer_writer_add_f32(writer, value);
    }

    template<>
    inline void serialize<double>(arora_buffer_writer *const writer, const double &value) noexcept
    {
      arora_buffer_writer_add_f64(writer, value);
    }

    template<>
    inline void serialize<std::string>(arora_buffer_writer *const writer, const std::string &value) noexcept
    {
      arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(value.data()), value.size());
    }

    template<>
    inline void serialize<std::string_view>(arora_buffer_writer *const writer, const std::string_view &value) noexcept
    {
      arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(value.data()), value.size());
    }
    
    template<>
    inline void arora_buffer_writer_add_bulk<std::uint8_t>(arora_buffer_writer *const writer, const std::uint8_t *const data, std::size_t size) noexcept {
      return arora_buffer_writer_add_u8_raw_bulk(writer, data, size);
    }
    
    template<>
    inline void arora_buffer_writer_add_bulk<std::uint16_t>(arora_buffer_writer *const writer, const std::uint16_t *const data, std::size_t size) noexcept {
      return arora_buffer_writer_add_u16_raw_bulk(writer, data, size);
    }
    
    template<>
    inline void arora_buffer_writer_add_bulk<std::uint32_t>(arora_buffer_writer *const writer, const std::uint32_t *const data, std::size_t size) noexcept {
      return arora_buffer_writer_add_u32_raw_bulk(writer, data, size);
    }
    
    template<>
    inline void arora_buffer_writer_add_bulk<std::uint64_t>(arora_buffer_writer *const writer, const std::uint64_t *const data, std::size_t size) noexcept {
      return arora_buffer_writer_add_u64_raw_bulk(writer, data, size);
    }
    template<>
    inline void arora_buffer_writer_add_bulk<std::int8_t>(arora_buffer_writer *const writer, const std::int8_t *const data, std::size_t size) noexcept {
      return arora_buffer_writer_add_i8_raw_bulk(writer, data, size);
    }
    
    template<>
    inline void arora_buffer_writer_add_bulk<std::int16_t>(arora_buffer_writer *const writer, const std::int16_t *const data, std::size_t size) noexcept {
      return arora_buffer_writer_add_i16_raw_bulk(writer, data, size);
    }
    
    template<>
    inline void arora_buffer_writer_add_bulk<std::int32_t>(arora_buffer_writer *const writer, const std::int32_t *const data, std::size_t size) noexcept {
      return arora_buffer_writer_add_i32_raw_bulk(writer, data, size);
    }
    
    template<>
    inline void arora_buffer_writer_add_bulk<std::int64_t>(arora_buffer_writer *const writer, const std::int64_t *const data, std::size_t size) noexcept {
      return arora_buffer_writer_add_i64_raw_bulk(writer, data, size);
    }

    template<>
    inline void arora_buffer_writer_add_bulk<float>(arora_buffer_writer *const writer, const float *const data, std::size_t size) noexcept {
      return arora_buffer_writer_add_f32_raw_bulk(writer, data, size);
    }
    
    template<>
    inline void arora_buffer_writer_add_bulk<double>(arora_buffer_writer *const writer, const double *const data, std::size_t size) noexcept {
      return arora_buffer_writer_add_f64_raw_bulk(writer, data, size);
    }
  }
}

#endif
