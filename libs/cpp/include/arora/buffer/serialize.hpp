#ifndef _ARORA_BUFFER_SERIALIZE_HPP_
#define _ARORA_BUFFER_SERIALIZE_HPP_

extern "C" {
  #include <arora/buffers.h>
}

#include <cstdint>
#include <optional>
#include <string_view>
#include <string>
#include "View.hpp"

namespace arora
{
  namespace buffer
  {
    template<typename T>
    void serialize(arora_buffer_writer *writer, const T &value);

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
    void serialize<std::uint64_t>(arora_buffer_writer *const writer, const std::uint32_t &value)
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
    void serialize<std::int64_t>(arora_buffer_writer *const writer, const std::int32_t &value)
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

    template<>
    void serialize<View<bool>>(arora_buffer_writer *const writer, const View<bool> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_BOOLEAN, value.size());
      arora_buffer_writer_add_boolean_raw_bulk(writer, value.data(), value.size());
    }

    template<size_t N>
    void serialize<std::array<bool, N>>(arora_buffer_writer *const writer, const std::array<bool, N> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_BOOLEAN, N);
      arora_buffer_writer_add_boolean_raw_bulk(writer, value.data(), N);
    }

    template<>
    void serialize<std::vector<bool>>(arora_buffer_writer *const writer, const std::vector<bool> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_BOOLEAN, value.size());
      arora_buffer_writer_add_boolean_raw_bulk(writer, value.data(), value.size());
    }

    template<>
    void serialize<View<std::uint8_t>>(arora_buffer_writer *const writer, const View<std::uint8_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U8, value.size());
      arora_buffer_writer_add_u8_raw_bulk(writer, value.data(), value.size());
    }

    template<size_t N>
    void serialize<std::array<std::uint8_t, N>>(arora_buffer_writer *const writer, const std::array<std::uint8_t, N> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U8, N);
      arora_buffer_writer_add_u8_raw_bulk(writer, value.data(), N);
    }

    template<>
    void serialize<std::vector<std::uint8_t>>(arora_buffer_writer *const writer, const std::vector<std::uint8_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U8, value.size());
      arora_buffer_writer_add_u8_raw_bulk(writer, value.data(), value.size());
    }

    template<>
    void serialize<View<std::uint16_t>>(arora_buffer_writer *const writer, const View<std::uint16_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U16, value.size());
      arora_buffer_writer_add_u16_raw_bulk(writer, value.data(), value.size());
    }

    template<size_t N>
    void serialize<std::array<std::uint16_t, N>>(arora_buffer_writer *const writer, const std::array<std::uint16_t, N> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U16, N);
      arora_buffer_writer_add_u16_raw_bulk(writer, value.data(), N);
    }

    template<>
    void serialize<std::vector<std::uint16_t>>(arora_buffer_writer *const writer, const std::vector<std::uint16_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U16, value.size());
      arora_buffer_writer_add_u16_raw_bulk(writer, value.data(), value.size());
    }

    template<>
    void serialize<View<std::uint32_t>>(arora_buffer_writer *const writer, const View<std::uint32_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U32, value.size());
      arora_buffer_writer_add_u32_raw_bulk(writer, value.data(), value.size());
    }

    template<size_t N>
    void serialize<std::array<std::uint32_t, N>>(arora_buffer_writer *const writer, const std::array<std::uint32_t, N> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U32, N);
      arora_buffer_writer_add_u32_raw_bulk(writer, value.data(), N);
    }

    template<>
    void serialize<std::vector<std::uint32_t>>(arora_buffer_writer *const writer, const std::vector<std::uint32_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U32, value.size());
      arora_buffer_writer_add_u32_raw_bulk(writer, value.data(), value.size());
    }

    template<>
    void serialize<View<std::uint64_t>>(arora_buffer_writer *const writer, const View<std::uint64_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U64, value.size());
      arora_buffer_writer_add_u64_raw_bulk(writer, value.data(), value.size());
    }

    template<size_t N>
    void serialize<std::array<std::uint64_t, N>>(arora_buffer_writer *const writer, const std::array<std::uint64_t, N> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U64, N);
      arora_buffer_writer_add_u64_raw_bulk(writer, value.data(), N);
    }

    template<>
    void serialize<std::vector<std::uint64_t>>(arora_buffer_writer *const writer, const std::vector<std::uint64_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U64, value.size());
      arora_buffer_writer_add_u64_raw_bulk(writer, value.data(), value.size());
    }

    template<>
    void serialize<View<std::int8_t>>(arora_buffer_writer *const writer, const View<std::int8_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I8, value.size());
      arora_buffer_writer_add_i8_raw_bulk(writer, value.data(), value.size());
    }

    template<size_t N>
    void serialize<std::array<std::int8_t, N>>(arora_buffer_writer *const writer, const std::array<std::int8_t, N> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I8, N);
      arora_buffer_writer_add_i8_raw_bulk(writer, value.data(), N);
    }

    template<>
    void serialize<std::vector<std::int8_t>>(arora_buffer_writer *const writer, const std::vector<std::int8_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I8, value.size());
      arora_buffer_writer_add_i8_raw_bulk(writer, value.data(), value.size());
    }

    template<>
    void serialize<View<std::int16_t>>(arora_buffer_writer *const writer, const View<std::int16_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I16, value.size());
      arora_buffer_writer_add_i16_raw_bulk(writer, value.data(), value.size());
    }

    template<size_t N>
    void serialize<std::array<std::int16_t, N>>(arora_buffer_writer *const writer, const std::array<std::int16_t, N> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I16, N);
      arora_buffer_writer_add_i16_raw_bulk(writer, value.data(), N);
    }

    template<>
    void serialize<std::vector<std::int16_t>>(arora_buffer_writer *const writer, const std::vector<std::int16_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I16, value.size());
      arora_buffer_writer_add_i16_raw_bulk(writer, value.data(), value.size());
    }

    template<>
    void serialize<View<std::int32_t>>(arora_buffer_writer *const writer, const View<std::int32_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I32, value.size());
      arora_buffer_writer_add_i32_raw_bulk(writer, value.data(), value.size());
    }

    template<size_t N>
    void serialize<std::array<std::int32_t, N>>(arora_buffer_writer *const writer, const std::array<std::int32_t, N> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I32, N);
      arora_buffer_writer_add_i32_raw_bulk(writer, value.data(), N);
    }

    template<>
    void serialize<std::vector<std::int32_t>>(arora_buffer_writer *const writer, const std::vector<std::int32_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I32, value.size());
      arora_buffer_writer_add_i32_raw_bulk(writer, value.data(), value.size());
    }

    template<>
    void serialize<View<std::int64_t>>(arora_buffer_writer *const writer, const View<std::int64_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I64, value.size());
      arora_buffer_writer_add_i64_raw_bulk(writer, value.data(), value.size());
    }

    template<size_t N>
    void serialize<std::array<std::int64_t, N>>(arora_buffer_writer *const writer, const std::array<std::int64_t, N> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I64, N);
      arora_buffer_writer_add_i64_raw_bulk(writer, value.data(), N);
    }

    template<>
    void serialize<std::vector<std::int64_t>>(arora_buffer_writer *const writer, const std::vector<std::int64_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I64, value.size());
      arora_buffer_writer_add_i64_raw_bulk(writer, value.data(), value.size());
    }

    template<>
    void serialize<View<float>>(arora_buffer_writer *const writer, const View<float> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_R32, value.size());
      arora_buffer_writer_add_r32_raw_bulk(writer, value.data(), value.size());
    }

    template<size_t N>
    void serialize<std::array<float, N>>(arora_buffer_writer *const writer, const std::array<float, N> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_R32, N);
      arora_buffer_writer_add_r32_raw_bulk(writer, value.data(), N);
    }

    template<>
    void serialize<std::vector<float>>(arora_buffer_writer *const writer, const std::vector<float> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_R32, value.size());
      arora_buffer_writer_add_r32_raw_bulk(writer, value.data(), value.size());
    }

    template<>
    void serialize<View<double>>(arora_buffer_writer *const writer, const View<double> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_R64, value.size());
      arora_buffer_writer_add_r64_raw_bulk(writer, value.data(), value.size());
    }

    template<size_t N>
    void serialize<std::array<double, N>>(arora_buffer_writer *const writer, const std::array<double, N> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_R64, N);
      arora_buffer_writer_add_r64_raw_bulk(writer, value.data(), N);
    }

    template<>
    void serialize<std::vector<double>>(arora_buffer_writer *const writer, const std::vector<double> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_R64, value.size());
      arora_buffer_writer_add_r64_raw_bulk(writer, value.data(), value.size());
    }

    template<>
    void serialize<View<std::string>>(arora_buffer_writer *const writer, const View<std::string> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_STRING, value.size());
      for (const auto &str : value)
      {
        arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(str.data()));
      }
    }

    template<size_t N>
    void serialize<std::array<std::string, N>>(arora_buffer_writer *const writer, const std::array<std::string, N> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_STRING, N);
      for (const auto &str : value)
      {
        arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(str.data()));
      }
    }

    template<>
    void serialize<std::vector<std::string>>(arora_buffer_writer *const writer, const std::vector<std::string> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_STRING, value.size());
      for (const auto &str : value)
      {
        arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(str.data()));
      }
    }

    template<>
    void serialize<View<std::string_view>>(arora_buffer_writer *const writer, const View<std::string> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_STRING, value.size());
      for (const auto &str : value)
      {
        arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(str.data()));
      }
    }

    template<size_t N>
    void serialize<std::array<std::string_view, N>>(arora_buffer_writer *const writer, const std::array<std::string, N> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_STRING, N);
      for (const auto &str : value)
      {
        arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(str.data()));
      }
    }

    template<>
    void serialize<std::vector<std::string_view>>(arora_buffer_writer *const writer, const std::vector<std::string> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_STRING, value.size());
      for (const auto &str : value)
      {
        arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(str.data()));
      }
    }
  }
}

#endif