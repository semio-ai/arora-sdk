#include <nao.hpp>
#include <iostream>
#include <string>
#include <qi/session.hpp>
#include <qi/registration.hpp>

int main() {
  std::cout << "Hello, it was NAO!" << std::endl;
  return 0;
}

std::string hello_nao() {
  qi::registerBaseTypes();
  qi::Session session;
  session.connect("tcp://localhost:9559");
  qi::AnyObject tts = session.service("ALTextToSpeech").value();
  tts.call<void>("say", "Hello, I'm NAO!");
  return "Hello, it was NAO!";
}
