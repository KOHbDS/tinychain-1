"""A `BTree` with a schema of named, :class:`Value`-typed :class:`Column` s."""

from ..state.generic import Map, Tuple
from ..state.number import UInt
from ..state.ref import Ref
from ..state import State, Stream
from ..util import form_of, to_json, uri, URI

from .base import Collection
from .bound import Range


class Schema(object):
    """A :class:`BTree` schema which comprises a tuple of :class:`Column` s."""

    def __init__(self, columns):
        self.columns = columns

    def __json__(self):
        return to_json(self.columns)


class BTree(Collection):
    """A `BTree` with a schema of named, :class:`Value`-typed :class:`Column` s."""

    __uri__ = uri(Collection) + "/btree"

    def __getitem__(self, prefix):
        """
        Return a slice of this `BTree` containing all keys which begin with the given prefix.
        """

        if not isinstance(prefix, Tuple) and not isinstance(prefix, tuple):
            prefix = (prefix,)

        range = _handle_range(prefix)
        return self._post("", {"range": range}, BTree)

    def count(self, range=None):
        """
        Return the number of keys in this `BTree`.

        To count the number of keys beginning with a specific prefix, call `btree[prefix].count()`.
        """

        range = _handle_range(range)
        return self._get("count", range, UInt)

    def delete(self, range=None):
        """
        Delete the contents of this `BTree` within the specified range.

        If no range is specified, the entire contents of this `BTree` will be deleted.
        """

        range = _handle_range(range)
        return self._delete("", range)

    def first(self):
        """
        Return the first row in this `BTree`.

        If there are no rows, this will raise a :class:`NotFoundError`.
        """

        return self._get("first", rtype=Map)

    def insert(self, key):
        """
        Insert the given key into this `BTree`.

        If the key is already present, this is a no-op.
        """

        return self._put("", value=key)

    def keys(self, range=None):
        """Return a :class:`Stream` of the keys in this `BTree` within the given range (if specified)."""

        range = _handle_range(range)
        return self._get("keys", range, Stream)

    def reverse(self):
        """
        Return a slice of this `BTree` with the same range but with its keys in reverse order.
        """

        return self._get("reverse", rtype=BTree)


def _handle_range(range):
    if range is None or isinstance(range, Ref) or isinstance(range, URI):
        return range

    if isinstance(range, State):
        form = form_of(range)
        if isinstance(form, list) or isinstance(form, tuple):
            range = form
        else:
            return range

    return [Range.from_slice(k) if isinstance(k, slice) else k for k in range]
