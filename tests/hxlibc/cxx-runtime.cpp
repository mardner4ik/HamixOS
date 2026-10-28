#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <map>
#include <memory>
#include <mutex>
#include <numeric>
#include <regex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

static int failures;

static void check(bool ok, const std::string &what)
{
    std::cout << (ok ? "PASS " : "FAIL ") << what << std::endl;
    if (!ok)
        failures++;
}

struct Tracked {
    static inline int alive = 0;
    Tracked() { alive++; }
    ~Tracked() { alive--; }
};

static int thrower(int depth)
{
    Tracked guard;
    if (depth == 0)
        throw std::runtime_error("deep");
    return thrower(depth - 1) + 1;
}

int main()
{
    try {
        thrower(10);
        check(false, "c++ exception unwinding");
    } catch (const std::runtime_error &e) {
        check(std::string(e.what()) == "deep" && Tracked::alive == 0, "c++ exception unwinding through 11 frames runs destructors");
    }

    std::map<std::string, int> counts;
    for (auto word : { "hamix", "bash", "hamix", "nook" })
        counts[word]++;
    check(counts["hamix"] == 2 && counts.size() == 3, "std::map and std::string");

    std::vector<int> values(1000);
    std::iota(values.begin(), values.end(), 1);
    std::sort(values.begin(), values.end(), std::greater<int>());
    check(values.front() == 1000 && std::accumulate(values.begin(), values.end(), 0L) == 500500, "std::vector, sort, accumulate");

    std::atomic<int> total { 0 };
    std::mutex lock;
    std::vector<std::thread> workers;
    std::vector<int> order;
    for (int t = 0; t < 4; t++)
        workers.emplace_back([&, t] {
            for (int i = 0; i < 1000; i++)
                total++;
            std::lock_guard<std::mutex> hold(lock);
            order.push_back(t);
        });
    for (auto &w : workers)
        w.join();
    check(total == 4000 && order.size() == 4, "std::thread, std::atomic, std::mutex");

    std::ostringstream text;
    text << "pi=" << 3.25 << " hex=" << std::hex << 255;
    check(text.str() == "pi=3.25 hex=ff", "std::ostringstream formatting (" + text.str() + ")");
    char buf[64];
    std::snprintf(buf, sizeof buf, "%.*g|%x", 6, 3.25, 255);
    check(std::string(buf) == "3.25|ff", std::string("snprintf %.*g and %x (") + buf + ")");

    std::regex pattern("([a-z]+)@([a-z]+)");
    std::smatch match;
    std::string address = "user@hamix";
    check(std::regex_match(address, match, pattern) && match[2] == "hamix", "std::regex");

    auto path = std::filesystem::temp_directory_path() / "cxx-test.txt";
    {
        std::ofstream out(path);
        out << "line one\nline two\n";
    }
    std::ifstream in(path);
    std::string first;
    std::getline(in, first);
    check(first == "line one" && std::filesystem::file_size(path) == 18, "std::fstream and std::filesystem");
    std::filesystem::remove(path);

    auto start = std::chrono::steady_clock::now();
    std::this_thread::sleep_for(std::chrono::milliseconds(50));
    auto waited = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - start).count();
    check(waited >= 45, "std::chrono::steady_clock and sleep_for");

    auto shared = std::make_shared<std::string>("hxlibc");
    std::weak_ptr<std::string> weak = shared;
    shared.reset();
    check(weak.expired(), "std::shared_ptr and weak_ptr");

    std::cout << "c++ runtime: " << failures << " failure(s)" << std::endl;
    return failures ? 1 : 0;
}
