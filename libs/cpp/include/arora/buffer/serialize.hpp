#ifndef _ARORA_BUFFER_SERIALIZE_HPP_
#define _ARORA_BUFFER_SERIALIZE_HPP_

extern "C" {
  #include <arora/buffers.h>
}

#include <cstdint>
#include <optional>
#include <string_view>
#include <string>
#include <vector>
#include "View.hpp"

namespace arora
{
  namespace buffer
  {
    template<typename T>
    void serialize(arora_buffer_writer *writer, const T &value);

    void serializeUnit(arora_buffer_writer *writer);

    template<>
    void serialize<bool>(arora_buffer_writer *const writer, const bool &value);

    template<>
    void serialize<std::uint8_t>(arora_buffer_writer *const writer, const std::uint8_t &value);

    template<>
    void serialize<std::uint16_t>(arora_buffer_writer *const writer, const std::uint16_t &value);

    template<>
    void serialize<std::uint32_t>(arora_buffer_writer *const writer, const std::uint32_t &value);

    template<>
    void serialize<std::uint64_t>(arora_buffer_writer *const writer, const std::uint64_t &value);

    template<>
    void serialize<std::int8_t>(arora_buffer_writer *const writer, const std::int8_t &value);

    template<>
    void serialize<std::int16_t>(arora_buffer_writer *const writer, const std::int16_t &value);

    template<>
    void serialize<std::int32_t>(arora_buffer_writer *const writer, const std::int32_t &value);

    template<>
    void serialize<std::int64_t>(arora_buffer_writer *const writer, const std::int64_t &value);

    template<>
    void serialize<float>(arora_buffer_writer *const writer, const float &value);

    template<>
    void serialize<double>(arora_buffer_writer *const writer, const double &value);

    template<>
    void serialize<std::string>(arora_buffer_writer *const writer, const std::string &value);

    template<>
    void serialize<std::string_view>(arora_buffer_writer *const writer, const std::string_view &value);

    template<>
    void serialize<View<bool>>(arora_buffer_writer *const writer, const View<bool> &value);

    template<>
    void serialize<View<std::uint8_t>>(arora_buffer_writer *const writer, const View<std::uint8_t> &value);

    template<>
    void serialize<std::vector<std::uint8_t>>(arora_buffer_writer *const writer, const std::vector<std::uint8_t> &value);

    template<>
    void serialize<View<std::uint16_t>>(arora_buffer_writer *const writer, const View<std::uint16_t> &value);

    template<>
    void serialize<std::vector<std::uint16_t>>(arora_buffer_writer *const writer, const std::vector<std::uint16_t> &value);

    template<>
    void serialize<View<std::uint32_t>>(arora_buffer_writer *const writer, const View<std::uint32_t> &value);

    template<>
    void serialize<std::vector<std::uint32_t>>(arora_buffer_writer *const writer, const std::vector<std::uint32_t> &value);

    template<>
    void serialize<View<std::uint64_t>>(arora_buffer_writer *const writer, const View<std::uint64_t> &value);

    template<>
    void serialize<std::vector<std::uint64_t>>(arora_buffer_writer *const writer, const std::vector<std::uint64_t> &value);

    template<>
    void serialize<View<std::int8_t>>(arora_buffer_writer *const writer, const View<std::int8_t> &value);

    template<>
    void serialize<std::vector<std::int8_t>>(arora_buffer_writer *const writer, const std::vector<std::int8_t> &value);

    template<>
    void serialize<View<std::int16_t>>(arora_buffer_writer *const writer, const View<std::int16_t> &value);

    template<>
    void serialize<std::vector<std::int16_t>>(arora_buffer_writer *const writer, const std::vector<std::int16_t> &value);

    template<>
    void serialize<View<std::int32_t>>(arora_buffer_writer *const writer, const View<std::int32_t> &value);

    template<>
    void serialize<std::vector<std::int32_t>>(arora_buffer_writer *const writer, const std::vector<std::int32_t> &value);

    template<>
    void serialize<View<std::int64_t>>(arora_buffer_writer *const writer, const View<std::int64_t> &value);

    template<>
    void serialize<std::vector<std::int64_t>>(arora_buffer_writer *const writer, const std::vector<std::int64_t> &value);

    template<>
    void serialize<View<float>>(arora_buffer_writer *const writer, const View<float> &value);

    template<>
    void serialize<std::vector<float>>(arora_buffer_writer *const writer, const std::vector<float> &value);

    template<>
    void serialize<View<double>>(arora_buffer_writer *const writer, const View<double> &value);

    template<>
    void serialize<std::vector<double>>(arora_buffer_writer *const writer, const std::vector<double> &value);

    template<>
    void serialize<View<std::string>>(arora_buffer_writer *const writer, const View<std::string> &value);

    template<>
    void serialize<std::vector<std::string>>(arora_buffer_writer *const writer, const std::vector<std::string> &value);

    template<>
    void serialize<View<std::string_view>>(arora_buffer_writer *const writer, const View<std::string_view> &value);

    template<>
    void serialize<std::vector<std::string_view>>(arora_buffer_writer *const writer, const std::vector<std::string_view> &value);
  }
}

#endif