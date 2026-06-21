class Handler:
    def __init__(self, name):
        self.name = name

    def handle(self, req):
        return self.process(req)

    def process(self, req):
        return req

def main():
    h = Handler()
    return h.handle(None)
