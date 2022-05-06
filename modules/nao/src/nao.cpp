#include <iostream>
#include <qi/session.hpp>
#include <qi/registration.hpp>

int main() {
  qi::registerBaseTypes();
  qi::Session session;
  session.connect("tcp://192.168.0.182:9559");
  qi::AnyObject tts = session.service("ALTextToSpeech").value();
  tts.call<void>("say", "Hello, I'm NAO!");
  std::cout << "Hello, it was NAO!" << std::endl;
  return 0;
}
