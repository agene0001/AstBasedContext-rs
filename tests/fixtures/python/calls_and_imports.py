from simple import Greeter, hello, add


def main():
    hello()
    result = add(1, 2)
    g = Greeter("Alice")
    g.greet()
    print(result)


class Runner:
    def run(self):
        main()
        self.helper()

    def helper(self):
        pass


if __name__ == "__main__":
    main()
