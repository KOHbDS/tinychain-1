from __future__ import annotations

import tinychain as tc
import unittest

from testutils import start_host


ENDPOINT = "/transact/hypothetical"
AREA_SERVICE = "http://127.0.0.1:8702/app/area"


@tc.get_op
def loop(until: tc.Number) -> tc.Int:
    @tc.closure(until)
    @tc.post_op
    def cond(i: tc.Int):
        return i < until

    @tc.post_op
    def step(i: tc.Int) -> tc.Int:
        return tc.Map(i=i + 1)  # here we return the new state of the loop

    initial_state = tc.Map(i=0)

    return tc.While(cond, step, initial_state)


@tc.get_op
def to_feet(_txn, meters: tc.Number) -> tc.Number:
    return tc.If(
        meters >= 0,
        meters * 3.28,
        tc.error.BadRequest("negative distance is not supported"))


# Specifying `metaclass=tc.Meta` provides JSON encoding functionality for user-defined classes.
# It's only required when subclassing a native class--subclasses of `Distance` automatically inherit its metaclass.
class Distance(tc.Number, metaclass=tc.Meta):
    __uri__ = tc.URI(AREA_SERVICE) + "/Distance"

    @tc.get_method
    def to_feet(self) -> Feet:
        return tc.error.NotImplemented("abstract")

    @tc.get_method
    def to_meters(self) -> Meters:
        return tc.error.NotImplemented("abstract")


class Feet(Distance):
    __uri__ = tc.URI(AREA_SERVICE) + "/Feet"

    @tc.get_method
    def to_feet(self) -> Feet:
        return self

    @tc.get_method
    def to_meters(self) -> Meters:
        return self / 3.28


class Meters(Distance):
    __uri__ = tc.URI(AREA_SERVICE) + "/Meters"

    @tc.get_method
    def to_feet(self) -> Feet:
        return self * 3.28

    @tc.get_method
    def to_meters(self) -> Meters:
        return self


class AreaService(tc.Cluster):
    __uri__ = tc.URI(AREA_SERVICE)

    def _configure(self):
        # make sure to include your app's classes here so your clients can find them!
        self.Distance = Distance
        self.Feet = Feet
        self.Meters = Meters

    @tc.post_method
    def area(self, txn, length: Distance, width: Distance) -> tc.Number:
        txn.length_m = length.to_meters()
        txn.width_m = width.to_meters()
        return txn.length_m * txn.width_m


class ClientService(tc.Cluster):
    __uri__ = tc.URI("http://127.0.0.1:8702/app/clientservice")

    @tc.get_method
    def room_area(self, txn, dimensions: tc.Tuple) -> Meters:
        area_service = tc.use(AreaService)
        txn.length = area_service.Meters(dimensions[0])
        txn.width = area_service.Meters(dimensions[1])
        return area_service.area({"length": txn.length, "width": txn.width})


class ClientDocTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.host = start_host("test_client_docs", [AreaService, ClientService])

    def testHello(self):
        hello = "Hello, World!"
        self.assertEqual(self.host.get(tc.uri(tc.String), hello), hello)
        self.assertEqual(self.host.post(ENDPOINT, tc.String(hello)), hello)

    def testWhile(self):
        cxt = tc.Context()
        cxt.loop = loop
        cxt.result = cxt.loop(10)
        self.assertEqual(self.host.post(ENDPOINT, cxt), {"i": 10})

    def testAreaService(self):
        service = tc.use(AreaService)
        params = {"length": service.Meters(5), "width": service.Meters(2)}
        self.assertEqual(self.host.post("/app/area/area", params), 10)

    def testClientService(self):
        self.assertEqual(self.host.get("/app/clientservice/room_area", (5, 10)), 50)

    def testTable(self):
        @tc.post_op
        def num_rows(txn):
            max_len = 100
            schema = tc.table.Schema(
                [tc.Column("user_id", tc.Number)],
                [tc.Column("name", tc.String, max_len), tc.Column("email", tc.String, max_len)])

            txn.table = tc.table.Table(schema)
            txn.insert = txn.table.insert((123,), ("Bob", "bob.roberts@example.com"))
            return tc.After(
                txn.insert,
                txn.table.count())

        actual = self.host.post(ENDPOINT, tc.form_of(num_rows))
        expected = 1
        self.assertEqual(actual, expected)

    @classmethod
    def tearDownClass(cls):
        cls.host.stop()


if __name__ == "__main__":
    unittest.main()
