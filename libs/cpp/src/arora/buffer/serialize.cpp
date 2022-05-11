#include "arora/buffer/serialize.hpp"

namespace arora
{
  namespace buffer
  {
    void serializeUnit(arora_buffer_writer *writer)
    {
      arora_buffer_writer_add_unit(writer);
    }

    template <>
    void serialize<bool>(arora_buffer_writer *const writer, const bool &value)
    {
      arora_buffer_writer_add_boolean(writer, value);
    }

    template <>
    void serialize<std::uint8_t>(arora_buffer_writer *const writer, const std::uint8_t &value)
    {
      arora_buffer_writer_add_u8(writer, value);
    }

    template <>
    void serialize<std::uint16_t>(arora_buffer_writer *const writer, const std::uint16_t &value)
    {
      arora_buffer_writer_add_u16(writer, value);
    }

    template <>
    void serialize<std::uint32_t>(arora_buffer_writer *const writer, const std::uint32_t &value)
    {
      arora_buffer_writer_add_u32(writer, value);
    }

    template <>
    void serialize<std::uint64_t>(arora_buffer_writer *const writer, const std::uint64_t &value)
    {
      arora_buffer_writer_add_u64(writer, value);
    }

    template <>
    void serialize<std::int8_t>(arora_buffer_writer *const writer, const std::int8_t &value)
    {
      arora_buffer_writer_add_i8(writer, value);
    }

    template <>
    void serialize<std::int16_t>(arora_buffer_writer *const writer, const std::int16_t &value)
    {
      arora_buffer_writer_add_i16(writer, value);
    }

    template <>
    void serialize<std::int32_t>(arora_buffer_writer *const writer, const std::int32_t &value)
    {
      arora_buffer_writer_add_i32(writer, value);
    }

    template <>
    void serialize<std::int64_t>(arora_buffer_writer *const writer, const std::int64_t &value)
    {
      arora_buffer_writer_add_i64(writer, value);
    }

    template <>
    void serialize<float>(arora_buffer_writer *const writer, const float &value)
    {
      arora_buffer_writer_add_f32(writer, value);
    }

    template <>
    void serialize<double>(arora_buffer_writer *const writer, const double &value)
    {
      arora_buffer_writer_add_f64(writer, value);
    }

    template <>
    void serialize<std::string>(arora_buffer_writer *const writer, const std::string &value)
    {
      arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(value.data()), value.size());
    }

    template <>
    void serialize<std::string>(arora_buffer_writer *const writer, const std::string &value)
    {
      arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(value.data()), value.size());
    }

    template <>
    void serialize<std::vector<bool>>(arora_buffer_writer *const writer, const std::vector<bool> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_BOOLEAN, value.size());
      arora_buffer_writer_add_boolean_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::uint8_t>>(arora_buffer_writer *const writer, const std::vector<std::uint8_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U8, value.size());
      arora_buffer_writer_add_u8_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::uint8_t>>(arora_buffer_writer *const writer, const std::vector<std::uint8_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U8, value.size());
      arora_buffer_writer_add_u8_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::uint16_t>>(arora_buffer_writer *const writer, const std::vector<std::uint16_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U16, value.size());
      arora_buffer_writer_add_u16_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::uint16_t>>(arora_buffer_writer *const writer, const std::vector<std::uint16_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U16, value.size());
      arora_buffer_writer_add_u16_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::uint32_t>>(arora_buffer_writer *const writer, const std::vector<std::uint32_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U32, value.size());
      arora_buffer_writer_add_u32_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::uint32_t>>(arora_buffer_writer *const writer, const std::vector<std::uint32_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U32, value.size());
      arora_buffer_writer_add_u32_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::uint64_t>>(arora_buffer_writer *const writer, const std::vector<std::uint64_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U64, value.size());
      arora_buffer_writer_add_u64_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::uint64_t>>(arora_buffer_writer *const writer, const std::vector<std::uint64_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_U64, value.size());
      arora_buffer_writer_add_u64_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::int8_t>>(arora_buffer_writer *const writer, const std::vector<std::int8_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I8, value.size());
      arora_buffer_writer_add_i8_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::int8_t>>(arora_buffer_writer *const writer, const std::vector<std::int8_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I8, value.size());
      arora_buffer_writer_add_i8_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::int16_t>>(arora_buffer_writer *const writer, const std::vector<std::int16_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I16, value.size());
      arora_buffer_writer_add_i16_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::int16_t>>(arora_buffer_writer *const writer, const std::vector<std::int16_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I16, value.size());
      arora_buffer_writer_add_i16_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::int32_t>>(arora_buffer_writer *const writer, const std::vector<std::int32_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I32, value.size());
      arora_buffer_writer_add_i32_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::int32_t>>(arora_buffer_writer *const writer, const std::vector<std::int32_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I32, value.size());
      arora_buffer_writer_add_i32_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::int64_t>>(arora_buffer_writer *const writer, const std::vector<std::int64_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I64, value.size());
      arora_buffer_writer_add_i64_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::int64_t>>(arora_buffer_writer *const writer, const std::vector<std::int64_t> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_I64, value.size());
      arora_buffer_writer_add_i64_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<float>>(arora_buffer_writer *const writer, const std::vector<float> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_F32, value.size());
      arora_buffer_writer_add_f32_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<float>>(arora_buffer_writer *const writer, const std::vector<float> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_F32, value.size());
      arora_buffer_writer_add_f32_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<double>>(arora_buffer_writer *const writer, const std::vector<double> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_F64, value.size());
      arora_buffer_writer_add_f64_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<double>>(arora_buffer_writer *const writer, const std::vector<double> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_F64, value.size());
      arora_buffer_writer_add_f64_raw_bulk(writer, value.data(), value.size());
    }

    template <>
    void serialize<std::vector<std::string>>(arora_buffer_writer *const writer, const std::vector<std::string> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_STRING, value.size());
      for (const auto &str : value)
      {
        arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(str.data()), str.size());
      }
    }

    template <>
    void serialize<std::vector<std::string>>(arora_buffer_writer *const writer, const std::vector<std::string> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_STRING, value.size());
      for (const auto &str : value)
      {
        arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(str.data()), str.size());
      }
    }

    template <>
    void serialize<std::vector<std::string>>(arora_buffer_writer *const writer, const std::vector<std::string> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_STRING, value.size());
      for (const auto &str : value)
      {
        arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(str.data()), str.size());
      }
    }

    template <>
    void serialize<std::vector<std::string>>(arora_buffer_writer *const writer, const std::vector<std::string> &value)
    {
      arora_buffer_writer_add_array_primitive(writer, ARORA_BUFFER_TYPE_STRING, value.size());
      for (const auto &str : value)
      {
        arora_buffer_writer_add_string(writer, reinterpret_cast<const std::uint8_t *>(str.data()), str.size());
      }
    }
  }
}