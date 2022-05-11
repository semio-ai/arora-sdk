#ifndef _ARORA_BUFFER_VIEW_HPP_
#define _ARORA_BUFFER_VIEW_HPP_

namespace arora
{
  namespace buffer
  {
    template<typename T>
    class View
    {
    public:
      using value_type = T;
      using size_type = std::size_t;

      View(const T *const data, const std::size_t size)
        : data_(data)
        , size_(size)
      {
      }

      View(const T *const begin, const T *const end)
        : data_(begin)
        , size_(std::distance(begin, end))
      {
      }

      const T *data() const
      {
        return data_;
      }

      const T *begin() const
      {
        return data_;
      }

      const T *end() const
      {
        return data_ + size_;
      }

      size_type size() const
      {
        return size_;
      }

      const T &operator[](const size_type index) const
      {
        return data_[index];
      }

      const T &at(const size_type index) const
      {
        return data_[index];
      }

      const T &front() const
      {
        return data_[0];
      }

      const T &back() const
      {
        return data_[size_ - 1];
      }

    private:
      const T *data_;
      std::size_t size_;
    };
  }
}

#endif // _ARORA_BUFFER_VIEW_HPP_
